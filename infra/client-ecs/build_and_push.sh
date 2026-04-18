#!/usr/bin/env bash
set -euo pipefail

AWS_PROFILE="${AWS_PROFILE:-personal}"
AWS_REGION="${AWS_REGION:-us-east-2}"
REPOSITORY_NAME="${REPOSITORY_NAME:-exchange-client}"
TAG="${1:-$(git rev-parse --short HEAD)}"

ACCOUNT_ID="$(AWS_PROFILE="$AWS_PROFILE" aws sts get-caller-identity --query Account --output text)"

if ! AWS_PROFILE="$AWS_PROFILE" aws ecr describe-repositories \
  --region "$AWS_REGION" \
  --repository-names "$REPOSITORY_NAME" >/dev/null 2>&1; then
  AWS_PROFILE="$AWS_PROFILE" aws ecr create-repository \
    --region "$AWS_REGION" \
    --repository-name "$REPOSITORY_NAME" >/dev/null
fi

REPOSITORY_URI="${ACCOUNT_ID}.dkr.ecr.${AWS_REGION}.amazonaws.com/${REPOSITORY_NAME}"

AWS_PROFILE="$AWS_PROFILE" aws ecr get-login-password --region "$AWS_REGION" | \
  docker login --username AWS --password-stdin "${ACCOUNT_ID}.dkr.ecr.${AWS_REGION}.amazonaws.com"

docker build -t "${REPOSITORY_NAME}:${TAG}" client
docker tag "${REPOSITORY_NAME}:${TAG}" "${REPOSITORY_URI}:${TAG}"
docker push "${REPOSITORY_URI}:${TAG}"

printf '%s\n' "${REPOSITORY_URI}:${TAG}"
