# The screenshot runner: Playwright's image (Chromium and its system
# libraries) plus the playwright package at the same version. The scripts
# (crawl.mjs, website.mjs and the files they import) are mounted at
# /runner/src by compose, so a change to them needs no rebuild.
FROM mcr.microsoft.com/playwright:v1.56.0-noble
WORKDIR /runner
COPY package.json ./
RUN npm install --omit=dev --no-audit --no-fund
# A throwaway screenshot-runner container for local end-to-end runs, never deployed.
# nosemgrep: dockerfile.security.missing-user-entrypoint.missing-user-entrypoint
ENTRYPOINT ["node"]
CMD ["/runner/src/crawl.mjs"]
