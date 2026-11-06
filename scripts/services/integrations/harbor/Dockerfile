# An Ubuntu 24.04 host for Harbor: systemd as the init system, sshd, and what
# a fresh server image from a cloud provider has (sudo, curl, certificates).
# Harbor provisions the rest. The test app is a Python program, so python3 is
# here too.
#
# Run it privileged with the host's cgroups (see scripts/services/integrations.yml):
# systemd needs them.
FROM jrei/systemd-ubuntu:24.04

RUN apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install --yes --no-install-recommends \
        ca-certificates curl gnupg iproute2 openssh-server python3 sudo \
    && rm -rf /var/lib/apt/lists/* \
    && mkdir -p /run/sshd /root/.ssh \
    && chmod 700 /root/.ssh \
    && systemctl enable ssh
