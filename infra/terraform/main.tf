# scrin-server on GCE (ADR-0008): one Container-Optimized OS VM with a static IP,
# TCP+UDP 443 open, a persistent data disk (SQLite + ACME cache) and the image from
# Artifact Registry. Relay/gateway are long-lived UDP/QUIC services, so not Cloud Run.

locals {
  repo_host  = "${var.region}-docker.pkg.dev"
  image      = "${local.repo_host}/${var.project_id}/${google_artifact_registry_repository.scrin.repository_id}/scrin-server:${var.image_tag}"
  data_mount = "/mnt/disks/scrin-data"
  labels     = { app = "scrin", component = "server" }
}

resource "google_project_service" "apis" {
  for_each           = toset(["compute.googleapis.com", "artifactregistry.googleapis.com", "dns.googleapis.com"])
  service            = each.value
  disable_on_destroy = false
}

resource "google_artifact_registry_repository" "scrin" {
  location      = var.region
  repository_id = "scrin"
  description   = "scrin container images"
  format        = "DOCKER"
  labels        = local.labels

  cleanup_policies {
    id     = "keep-recent"
    action = "KEEP"
    most_recent_versions {
      keep_count = 20
    }
  }

  depends_on = [google_project_service.apis]
}

resource "google_service_account" "server" {
  account_id   = var.name
  display_name = "scrin-server VM"
}

resource "google_artifact_registry_repository_iam_member" "pull" {
  location   = google_artifact_registry_repository.scrin.location
  repository = google_artifact_registry_repository.scrin.name
  role       = "roles/artifactregistry.reader"
  member     = "serviceAccount:${google_service_account.server.email}"
}

resource "google_project_iam_member" "logs" {
  for_each = toset(["roles/logging.logWriter", "roles/monitoring.metricWriter"])
  project  = var.project_id
  role     = each.value
  member   = "serviceAccount:${google_service_account.server.email}"
}

resource "google_compute_address" "server" {
  name         = var.name
  region       = var.region
  address_type = "EXTERNAL"
  network_tier = "PREMIUM"
  labels       = local.labels
  depends_on   = [google_project_service.apis]
}

resource "google_compute_firewall" "public" {
  name        = "${var.name}-public"
  network     = "default"
  description = "scrin-server: HTTPS/relay/WS gateway (443/tcp), WebTransport (443/udp), captive probe (80/tcp), QAD (7842/udp)"
  direction   = "INGRESS"
  priority    = 1000

  allow {
    protocol = "tcp"
    ports    = ["80", "443"]
  }
  allow {
    protocol = "udp"
    ports    = ["443", "7842"]
  }

  source_ranges = ["0.0.0.0/0"]
  target_tags   = [var.name]
  depends_on    = [google_project_service.apis]
}

resource "google_compute_firewall" "ssh" {
  count         = length(var.ssh_source_ranges) > 0 ? 1 : 0
  name          = "${var.name}-ssh"
  network       = "default"
  direction     = "INGRESS"
  source_ranges = var.ssh_source_ranges
  target_tags   = [var.name]

  allow {
    protocol = "tcp"
    ports    = ["22"]
  }
}

resource "google_compute_disk" "data" {
  name   = "${var.name}-data"
  zone   = var.zone
  type   = "pd-balanced"
  size   = var.data_disk_gb
  labels = local.labels
}

resource "google_compute_resource_policy" "snapshots" {
  name   = "${var.name}-daily"
  region = var.region

  snapshot_schedule_policy {
    schedule {
      daily_schedule {
        days_in_cycle = 1
        start_time    = "03:00"
      }
    }
    retention_policy {
      max_retention_days    = 14
      on_source_disk_delete = "KEEP_AUTO_SNAPSHOTS"
    }
  }
}

resource "google_compute_disk_resource_policy_attachment" "data" {
  name = google_compute_resource_policy.snapshots.name
  disk = google_compute_disk.data.name
  zone = var.zone
}

data "google_compute_image" "cos" {
  family  = "cos-stable"
  project = "cos-cloud"
}

resource "google_compute_instance" "server" {
  name         = var.name
  zone         = var.zone
  machine_type = var.machine_type
  tags         = [var.name]
  labels       = local.labels

  boot_disk {
    initialize_params {
      image = data.google_compute_image.cos.self_link
      size  = 20
      type  = "pd-balanced"
    }
  }

  attached_disk {
    source      = google_compute_disk.data.id
    device_name = "scrin-data"
  }

  network_interface {
    network = "default"
    access_config {
      nat_ip       = google_compute_address.server.address
      network_tier = "PREMIUM"
    }
  }

  service_account {
    email  = google_service_account.server.email
    scopes = ["cloud-platform"]
  }

  shielded_instance_config {
    enable_secure_boot          = true
    enable_vtpm                 = true
    enable_integrity_monitoring = true
  }

  metadata = {
    enable-oslogin            = "TRUE"
    google-logging-enabled    = "true"
    google-monitoring-enabled = "true"
    user-data = templatefile("${path.module}/cloud-init.yaml.tftpl", {
      image        = local.image
      repo_host    = local.repo_host
      data_mount   = local.data_mount
      domain       = var.domain
      acme_contact = var.acme_contact
      acme_staging = var.acme_staging
      gw_max_bps   = var.gw_max_bps
    })
  }

  # A new image_tag updates user-data in place; it takes effect on the next boot
  # (`gcloud compute instances reset <name>`; static IP and data disk are kept).
  allow_stopping_for_update = true

  depends_on = [
    google_artifact_registry_repository_iam_member.pull,
    google_compute_firewall.public,
  ]
}

resource "google_dns_record_set" "server" {
  count        = var.dns_managed_zone == "" ? 0 : 1
  project      = var.dns_project_id == "" ? var.project_id : var.dns_project_id
  managed_zone = var.dns_managed_zone
  name         = "${trimsuffix(var.domain, ".")}."
  type         = "A"
  ttl          = 300
  rrdatas      = [google_compute_address.server.address]
}
