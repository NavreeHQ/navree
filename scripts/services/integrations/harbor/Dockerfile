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

# sshd listens on 12222, the port the test gives Harbor (harbor.toml's
# [ssh] port) and the one Harbor's firewall keeps open: a server listening on
# another port would be shut out as soon as the firewall is on. The container
# publishes the same port. Ubuntu starts sshd from ssh.socket, which listens
# where the socket unit says and not where sshd_config does, so both name the
# port. The socket names both address families: with only a port number,
# systemd listens on the IPv6 wildcard alone here, and Docker reaches the
# container over IPv4 (the connection is reset during the key exchange).
RUN mkdir -p /etc/ssh/sshd_config.d /etc/systemd/system/ssh.socket.d \
    && printf 'Port 12222\n' > /etc/ssh/sshd_config.d/00-harbor-test.conf \
    && printf '[Socket]\nListenStream=\nListenStream=0.0.0.0:12222\nListenStream=[::]:12222\n' \
        > /etc/systemd/system/ssh.socket.d/harbor-test.conf

# The base image never starts systemd-user-sessions, so /run/nologin stays and
# every user but root is refused at login ("System is booting up"), the deploy
# user Harbor creates included. A real server image starts it at boot.
RUN mkdir -p /etc/systemd/system/multi-user.target.wants \
    && ln -s /usr/lib/systemd/system/systemd-user-sessions.service \
        /etc/systemd/system/multi-user.target.wants/systemd-user-sessions.service
