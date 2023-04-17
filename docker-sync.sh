#!/bin/bash

set -e

echo "Building Docker image..."
sudo docker build -t moviemail .

echo "Saving Docker image as a .tar file..."
sudo docker save moviemail -o moviemail.tar

echo "Changing ownership of the .tar file..."
sudo chown $(whoami):$(whoami) moviemail.tar

echo "Copying the .tar file to the remote host..."
scp moviemail.tar docker-host:/volume1/docker/moviemail/.

echo "Done."
