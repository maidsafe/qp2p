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
scp ./target/release/examples/client_node $droplet_ip:/home/qa/
ssh $droplet_ip "nohup /home/qa/client_node -b '$bootstrap_cfg' & tail -f nohup.out"
