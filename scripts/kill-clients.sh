
cfg=$1
lines=`cat ip_list`
i=0

for ip in $lines
do
  let i++

  echo "Killing client $i ($ip) ..."
  ssh -o "StrictHostKeyChecking no" qa@$ip "pkill client_node"
done
