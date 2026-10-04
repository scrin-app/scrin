# infra/terraform — official scrin-server on GCP

Implements ADR-0008 for the data plane: one Container-Optimized OS VM in `europe-west3` running
the `scrin-server` image (rendezvous + relay + gateway), a static IP, firewall for 443 tcp/udp
(+80/tcp captive probe, 7842/udp QAD), a persistent data disk with daily snapshots, an Artifact
Registry repository and an optional Cloud DNS `A` record.

Nothing here has been applied. Plan before apply, always:

```powershell
cd infra/terraform
copy terraform.tfvars.example terraform.tfvars   # fill in; gitignored
terraform init -backend-config="bucket=<state-bucket>" -backend-config="prefix=scrin/server"
terraform plan -out plan.tfplan
# review, then (owner only): terraform apply plan.tfplan
```

Release flow (clean tree, commit → push → deploy):

1. Build and push the image from a clean worktree to the `artifact_registry` output, tagged with
   the version (never `latest`).
2. Set `image_tag`, `terraform plan`/`apply`, then `gcloud compute instances reset scrin-server`
   so the VM boots with the new user-data.
3. Verify live: `curl https://<domain>/v1/info` reports the new `version`, and a real
   register → resolve round trip succeeds.

DNS for `scrin.dragoscatalin.ro` is managed outside this project today: leave
`dns_managed_zone` empty and create the `A` record from the `static_ip` output.
