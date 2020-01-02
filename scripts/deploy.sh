# Deploys client_node to a single droplet and runs it
#
# Usage: `./deploy.sh $bootstrap_config $remote_ip`
# e.g.: `./deploy.sh '{"peer_addr": ...}' 192.168.0.1`
#
# For multiple droplets you can use xargs: `cat ip_list | xargs ./deploy.sh '{"peer_addr": ...}'`

set -e

droplet_ip=$2
bootstrap_cfg=$1

cargo build --release --example client_node
rsync -avz -e "ssh -o StrictHostKeyChecking=no" --progress ./target/release/examples/client_node qa@$droplet_ip:/home/qa/
ssh -o "StrictHostKeyChecking no" qa@$droplet_ip "export RUST_LOG=client_node=trace RUST_BACKTRACE=1 ; pkill client_node ; truncate -s0 ./client-nohup.out ; (nohup /home/qa/client_node --hard-coded-contacts '$bootstrap_cfg' > ./client-nohup.out) & tail -f ./client-nohup.out"
