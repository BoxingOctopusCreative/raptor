# Signing key setup

1. Generate a key:
gpg --full-generate-key

2. Export for apt:
gpg --armor --export YOUR_KEY_ID | gpg --dearmor -o keyrings/repo.gpg

3. Sign Release when publishing:
raptor repo index --repo /home/ubuntu/raptor/examples/demo/repo
