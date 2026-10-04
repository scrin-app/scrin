variable "project_id" {
  description = "GCP project id that hosts scrin-server."
  type        = string
}

variable "region" {
  description = "Region for the VM, static IP and Artifact Registry."
  type        = string
  default     = "europe-west3"
}

variable "zone" {
  description = "Zone for the single scrin-server VM."
  type        = string
  default     = "europe-west3-a"
}

variable "name" {
  description = "Name prefix for every resource."
  type        = string
  default     = "scrin-server"
}

variable "machine_type" {
  description = "GCE machine type. Egress, not CPU, is the cost driver (ADR-0008)."
  type        = string
  default     = "e2-small"
}

variable "image_tag" {
  description = "scrin-server image tag in the Artifact Registry repo (an immutable tag or digest, never latest)."
  type        = string
}

variable "domain" {
  description = "Public name of the server, e.g. relay.scrin.dragoscatalin.ro. Used for ACME and the relay URL."
  type        = string
}

variable "acme_contact" {
  description = "ACME account contact, e.g. mailto:ops@example.org."
  type        = string
  default     = ""
}

variable "acme_staging" {
  description = "Use the Let's Encrypt staging directory (untrusted certs) while testing."
  type        = bool
  default     = false
}

variable "dns_managed_zone" {
  description = "Cloud DNS managed zone name for var.domain. Empty = DNS is managed elsewhere; create the A record by hand from the static_ip output."
  type        = string
  default     = ""
}

variable "dns_project_id" {
  description = "Project that owns the DNS zone, if different from project_id."
  type        = string
  default     = ""
}

variable "gw_max_bps" {
  description = "Per-session gateway bandwidth cap, bytes/second."
  type        = number
  default     = 3000000
}

variable "data_disk_gb" {
  description = "Persistent disk for SQLite + ACME cache."
  type        = number
  default     = 10
}

variable "ssh_source_ranges" {
  description = "CIDRs allowed to SSH (via IAP use 35.235.240.0/20). Empty = no SSH rule."
  type        = list(string)
  default     = ["35.235.240.0/20"]
}
