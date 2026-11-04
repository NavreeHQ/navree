// pm2's entry for the live theme demo (apps/theme-demo), run as forge. theme-demo.sh
// starts it the first time and reloads it after each build:
//
//   pm2 start /home/forge/navree.com/repo/deploy/navree.com/theme-demo.config.cjs && pm2 save
//
// The app listens on 127.0.0.1:8095 only; nginx passes /demo/ui/ and /_navree/ui/ to
// it (demo-ui.conf). It keeps nothing on disk and nothing between visitors, so a
// restart loses nothing.
module.exports = {
  apps: [
    {
      name: "navree-theme-demo",
      script: "/home/forge/navree.com/theme-demo/current/theme-demo",
      interpreter: "none",
      exec_mode: "fork",
      instances: 1,
      autorestart: true,
      max_restarts: 10,
      restart_delay: 2000,
      max_memory_restart: "256M",
      env: {
        PORT: "8095",
      },
    },
  ],
};
