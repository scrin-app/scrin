terraform {
  required_version = ">= 1.6"

  required_providers {
    google = {
      source  = "hashicorp/google"
      version = "~> 6.0"
    }
  }

  # Remote state: configure at init time, e.g.
  #   terraform init -backend-config="bucket=<state-bucket>" -backend-config="prefix=scrin/server"
  backend "gcs" {}
}

provider "google" {
  project = var.project_id
  region  = var.region
}
