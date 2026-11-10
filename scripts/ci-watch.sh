#!/usr/bin/env bash
# Watches the machine while a CI step runs and prints, into the step's own
# log, what would explain the step or the runner dying: the memory, the
# largest processes, the kernel's out-of-memory kills and the signals that end
# processes, with who sent them. An artifact or a later step is no help when
# the runner itself is ended, so everything goes to the output of the step.
#
#   setsid scripts/ci-watch.sh "$$" &
#
# at the top of a step's script follows the step's shell until it ends. It
# runs in a session of its own (setsid), so a signal sent to the step's
# process group does not silence it, and the runner keeps reading the step's
# output for a few seconds after the shell ends.
#
# Every NAVREE_CI_WATCH_INTERVAL seconds (default 5), one line:
#   [watch] 04:28:40 memory: 9876 MB used, 5432 MB available of 15990 MB; swap 0 of 4095 MB used; load 3.95 4.10 3.80
# Every minute, and at every such line while less than NAVREE_CI_WATCH_LOW_MB
# (default 2048) is available: the ten largest processes (pid, process group,
# resident memory, command) and the memory of each container.
#
# Within a second (Linux, with sudo and the kernel's tracing), lines that
# start with "[watch] trace:" and name the sending process and its pid first:
#   - a SIGTERM or SIGKILL sent to a whole process group or to every process
#     (kill(2) with a pid of 0 or below, in hex: 0 is the sender's own group,
#     0xfffec94e is group 79538, 0xffffffff is every process):
#       kill-24702 [001] ..... 512.3: sys_kill(pid: 0, sig: 0xf)
#   - a SIGHUP, SIGINT, SIGTERM or SIGKILL that reaches the step's shell or the
#     runner (Runner.Listener, Runner.Worker), whoever sent it:
#       kill-24702 [001] d..1. 512.3: signal_generate: sig=15 ... comm=Runner.Worker pid=2242 ...
#   - a process the kernel picks to end for want of memory (mark_victim).
# The kernel's own out-of-memory lines (dmesg) follow as "[watch] kernel:".
set -u

target=${1:?usage: ci-watch.sh PID, the shell of the step}
interval=${NAVREE_CI_WATCH_INTERVAL:-5}
[ "$interval" -ge 1 ] 2>/dev/null || interval=5
low_mb=${NAVREE_CI_WATCH_LOW_MB:-2048}

say() {
  printf '[watch] %s %s\n' "$(date -u +%H:%M:%S)" "$*"
}

available=0
memory() {
  local total used swap_used swap_total
  read -r total available swap_used swap_total < <(awk '
    /^MemTotal:/ { t = $2 } /^MemAvailable:/ { a = $2 }
    /^SwapTotal:/ { st = $2 } /^SwapFree:/ { sf = $2 }
    END { printf "%d %d %d %d\n", t / 1024, a / 1024, (st - sf) / 1024, st / 1024 }' /proc/meminfo)
  used=$((total - available))
  say "memory: $used MB used, $available MB available of $total MB;" \
    "swap $swap_used of $swap_total MB used; load $(cut -d ' ' -f 1-3 /proc/loadavg)"
}

processes() {
  ps -eo pid=,pgid=,rss=,args= --sort=-rss | head -n 10 | awk '{
    pid = $1; group = $2; mb = int($3 / 1024); $1 = $2 = $3 = ""; sub(/^ +/, "")
    printf "[watch]   pid %-7s group %-7s %6d MB  %.140s\n", pid, group, mb, $0 }'
  if command -v docker >/dev/null 2>&1 && [ -n "$(docker ps --quiet 2>/dev/null | head -n 1)" ]; then
    docker stats --no-stream --format '{{.Name}} {{.MemUsage}}' 2>/dev/null | sed 's/^/[watch]   container /'
  fi
}

# The kernel's out-of-memory lines not printed yet.
kernel_seen=0
kernel_lines() {
  local lines count
  lines=$(sudo -n dmesg 2>/dev/null |
    grep -E 'invoked oom-killer|Out of memory|oom-kill:|Killed process|oom_reaper' || true)
  [ -n "$lines" ] || return 0
  count=$(printf '%s\n' "$lines" | wc -l)
  if [ "$count" -gt "$kernel_seen" ]; then
    printf '%s\n' "$lines" | tail -n "+$((kernel_seen + 1))" | sed 's/^/[watch] kernel: /'
    kernel_seen=$count
  fi
}

# A trace instance of its own: its events and buffer are apart from any other
# tracing on the machine, and it is removed at the end.
trace=""
if [ "$(uname -s)" = Linux ] && sudo -n true 2>/dev/null; then
  for root in /sys/kernel/tracing /sys/kernel/debug/tracing; do
    if sudo -n test -d "$root/instances"; then
      trace="$root/instances/navree-watch-$$"
      break
    fi
  done
fi
# The trace lines printed so far.
seen=$(mktemp)

# Turns on one event with its filter; an event whose filter is refused stays
# off (unfiltered, signal_generate would print every SIGCHLD).
trace_event() {
  local dir="$trace/events/$1"
  if ! sudo -n test -d "$dir"; then
    say "the kernel has no $1 event: not traced"
    return
  fi
  if [ -n "$2" ] && ! printf '%s\n' "$2" | sudo -n tee "$dir/filter" >/dev/null 2>&1; then
    say "the kernel refused the filter of $1: not traced"
    return
  fi
  echo 1 | sudo -n tee "$dir/enable" >/dev/null 2>&1 || say "cannot turn on $1"
}

start_tracing() {
  if ! sudo -n mkdir "$trace" 2>/dev/null; then
    say "cannot make a trace instance: signals are not traced"
    trace=""
    return
  fi
  # The names in the trace lines come from a cache of recent processes (128
  # by default, the machine's setting); a sender no longer in it shows as
  # <...>, with its pid.
  echo 8192 | sudo -n tee "${trace%/instances/*}/saved_cmdlines_size" >/dev/null 2>&1
  # A negative pid is recorded as an unsigned number of 32 or 64 bits.
  trace_event syscalls/sys_enter_kill '(pid == 0 || pid >= 2147483648) && (sig == 9 || sig == 15)'
  # The runner by name: in a PID namespace (a container, WSL) the kernel
  # records pids this script does not see.
  local reaches="pid == $target || comm == \"Runner.Listener\" || comm == \"Runner.Worker\""
  trace_event signal/signal_generate "($reaches) && (sig == 1 || sig == 2 || sig == 9 || sig == 15)"
  trace_event oom/mark_victim ''
  say "tracing group-wide kills, signals to the step's shell (pid $target) and to the runner," \
    "and out-of-memory kills"
}

stop_tracing() {
  rm -f "$seen"
  [ -n "$trace" ] || return 0
  echo 0 | sudo -n tee "$trace/events/enable" >/dev/null 2>&1
  sudo -n rmdir "$trace" 2>/dev/null
  trace=""
}

# Prints the trace lines not printed yet. The buffer is read, not consumed, a
# moment after the events: read at once, a sender's name may not be known yet.
trace_lines() {
  [ -n "$trace" ] || return 0
  local now
  now=$(sudo -n cat "$trace/trace" 2>/dev/null | grep -v '^#')
  [ -n "$now" ] || return 0
  printf '%s\n' "$now" | grep -vxF -f "$seen" | sed 's/^ */[watch] trace: /'
  printf '%s\n' "$now" >"$seen"
}

# PIPE: the runner stops reading the step's output a few seconds after the
# shell ends.
trap 'stop_tracing; exit 0' TERM INT HUP PIPE

say "watching the machine while pid $target runs (every $interval s)"
[ -n "$trace" ] && start_tracing
kernel_lines
memory
processes
second=0
while kill -0 "$target" 2>/dev/null; do
  sleep 1
  trace_lines
  second=$((second + 1))
  if [ $((second % interval)) -eq 0 ]; then
    kernel_lines
    memory
    if [ $((second % 60)) -lt "$interval" ] || [ "$available" -lt "$low_mb" ]; then
      processes
    fi
  fi
done
say "pid $target has ended"
sleep 0.2
trace_lines
kernel_lines
memory
processes
stop_tracing
