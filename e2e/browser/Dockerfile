# The screenshot runner: Playwright's image (Chromium and its system
# libraries) plus the playwright package at the same version. The scripts
# (crawl.mjs and the files it imports) are mounted at
# /runner/src by compose, so a change to them needs no rebuild.
FROM mcr.microsoft.com/playwright:v1.56.0-noble
WORKDIR /runner
COPY package.json ./
RUN npm install --omit=dev --no-audit --no-fund
# The runner works as the image's own unprivileged user. It reads /runner and
# the browsers in /ms-playwright, which every user can read, keeps Chromium's
# profile in /tmp, and writes only to the /out mount, as the k6 container
# does as its own user.
USER pwuser
ENTRYPOINT ["node"]
CMD ["/runner/src/crawl.mjs"]
