FROM mcr.microsoft.com/dotnet/sdk:8.0-alpine AS build
WORKDIR /source

COPY *.csproj .
RUN dotnet restore

COPY . .
RUN dotnet publish --no-restore -o /app

FROM mcr.microsoft.com/dotnet/runtime:8.0-alpine
ARG UID=1001

RUN addgroup -S ffxiv && adduser -S ffxiv -G ffxiv --uid ${UID}
USER ffxiv

WORKDIR /app
COPY --from=build /app .
ENTRYPOINT ["/app/FFXIVDownloader"]