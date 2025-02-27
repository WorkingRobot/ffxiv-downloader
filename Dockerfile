FROM mcr.microsoft.com/dotnet/sdk:9.0-alpine AS build
WORKDIR /source

COPY . .
RUN dotnet publish -c Release --project FFXIVDownloader.Command -o /app

FROM mcr.microsoft.com/dotnet/runtime:9.0-alpine

WORKDIR /app
COPY --from=build /app .
ENTRYPOINT ["/app/FFXIVDownloader.Command"]