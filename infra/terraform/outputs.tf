output "static_ip" {
  description = "External IP of scrin-server; point the A record of var.domain here when DNS is managed elsewhere."
  value       = google_compute_address.server.address
}

output "image" {
  description = "Image the VM runs."
  value       = local.image
}

output "artifact_registry" {
  description = "docker push target prefix."
  value       = "${local.repo_host}/${var.project_id}/${google_artifact_registry_repository.scrin.repository_id}"
}

output "server_url" {
  description = "URL clients use as their scrin server."
  value       = "https://${trimsuffix(var.domain, ".")}"
}
