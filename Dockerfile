# Start with the .NET Core SDK image
FROM mcr.microsoft.com/dotnet/sdk:8.0 AS build

# Set the working directory
WORKDIR /app

# Copy the project files to the container
COPY . .

# Build the application
RUN dotnet build -c Release

# Publish the application
RUN dotnet publish -c Release -o out

# Start with a new image
FROM mcr.microsoft.com/dotnet/runtime-deps:8.0-alpine AS runtime

# Set the working directory
WORKDIR /app

# Copy the published output from the build image
COPY --from=build /app/out .

# Set the entry point for the container
ENTRYPOINT ["./FFXIVDownloader"]