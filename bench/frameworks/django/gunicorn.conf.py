# Gunicorn settings (https://gunicorn.org/reference/settings/).
import os

# os.process_cpu_count() honours the container's cpuset (8 in the benchmark);
# multiprocessing.cpu_count() would report every host CPU.
cpus = os.process_cpu_count() or 1

wsgi_app = "benchsite.wsgi:application"
bind = "0.0.0.0:8080"

# Gunicorn's starting point is (2 x cores) + 1 workers. gthread rather than the
# default sync worker because clients connect directly with keep-alive (no
# buffering proxy in front); see README.md.
worker_class = "gthread"
workers = 2 * cpus + 1
# Django holds one persistent connection per thread: stay within 64 in total.
threads = max(1, 64 // workers)

# Heartbeat files on tmpfs instead of the container's overlay filesystem.
worker_tmp_dir = "/dev/shm"

# No access log (the default); errors go to stderr.
accesslog = None
