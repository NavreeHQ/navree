# syntax=docker/dockerfile:1
FROM golang:1.27-bookworm AS build
WORKDIR /src
COPY go.mod go.sum ./
RUN --mount=type=cache,target=/go/pkg/mod go mod download
COPY *.go ./
RUN --mount=type=cache,target=/go/pkg/mod --mount=type=cache,target=/root/.cache/go-build \
    CGO_ENABLED=0 go build -trimpath -ldflags="-s -w" -o /out/server .

FROM debian:bookworm-slim
COPY --from=build /out/server /usr/local/bin/server
ENV GIN_MODE=release
USER nobody
EXPOSE 8080
CMD ["/usr/local/bin/server"]
