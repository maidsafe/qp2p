# Deploys bootstrap_node to a single droplet and runs it

# Usage: `./deploy-bootstrap.sh`
# It will print the bootstrap node connection information to stdout.
# It is safe to terminate this program - the bootstrap node will keep running.

set -e
droplet_ip=$1

cargo build --release --example bootstrap_node

# replace the IP address in the config and write it to the droplet
cat bootstrap.config | jq -c ".ip = \"$droplet_ip\"" | ssh -o "StrictHostKeyChecking no" qa@$droplet_ip "cat >/home/qa/bootstrap.config"
rsync -avz -e "ssh -o StrictHostKeyChecking=no" --progress ./target/release/examples/bootstrap_node qa@$droplet_ip:/home/qa/
ssh -o "StrictHostKeyChecking no" qa@$droplet_ip "pkill bootstrap_node ; export RUST_LOG=bootstrap_node=trace ; /home/qa/bootstrap_node"
