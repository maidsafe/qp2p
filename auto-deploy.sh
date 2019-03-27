
cfg=$1
lines=`cat ip_list`
i=0

tmux new-session -s deploy -d

for ip in $lines
do
  let i++

  echo "Running client $i ($ip) ..."
  tmux new-window -t deploy:$i -n "$i-$ip"
  tmux send-keys -t deploy:$i "./deploy.sh '$cfg' $ip" C-m
done
