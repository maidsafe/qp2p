#!/usr/bin/env bash

# Usage: `./auto-test.sh '{"peer_addr": ...}'` (insert CrustInfo from the boostrap node)
# To kill the clients: `tmux kill-session -t crust`

tmux new-session -s crust -d

for i in {1..50}
do
  echo "Running client $i ..."
  tmux new-window -t crust:$i -n "client$i"
  tmux send-keys -t crust:$i "./target/release/examples/client_node -b '$1'" C-m
done
