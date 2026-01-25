default:
  just --list

# Build the docker image
build-image:
    docker build -t coder-mcp .
