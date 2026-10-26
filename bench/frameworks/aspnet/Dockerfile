# syntax=docker/dockerfile:1
# .NET 10 (LTS). Microsoft publishes .NET 10 images on Ubuntu 24.04 (glibc), not Debian.
FROM mcr.microsoft.com/dotnet/sdk:10.0 AS build
WORKDIR /src
ENV DOTNET_CLI_TELEMETRY_OPTOUT=1 DOTNET_NOLOGO=1 \
    DOTNET_GCHeapHardLimit=0x80000000
COPY Bench.csproj packages.lock.json ./
RUN --mount=type=cache,target=/root/.nuget/packages dotnet restore --locked-mode
COPY . .
RUN --mount=type=cache,target=/root/.nuget/packages \
    dotnet publish -c Release --no-restore -o /out

FROM mcr.microsoft.com/dotnet/aspnet:10.0
WORKDIR /app
COPY --from=build /out ./
# The image already listens on 8080 (ASPNETCORE_HTTP_PORTS) and runs as a non-root user.
USER $APP_UID
ENTRYPOINT ["dotnet", "Bench.dll"]
