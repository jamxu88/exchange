#!/usr/bin/env bash
set -euo pipefail

AWS_PROFILE="${AWS_PROFILE:-personal}"
AWS_REGION="${AWS_REGION:-us-east-2}"
STACK_NAME="${STACK_NAME:-exchange-client}"
PROJECT_NAME="${PROJECT_NAME:-exchange-client}"
DOMAIN_NAME="${DOMAIN_NAME:-exchange.jamesxu.dev}"
MARKETS="${MARKETS:-}"
EXCHANGE_SERVER_URL="${EXCHANGE_SERVER_URL:-https://exchange.jamesxu.dev}"
BACKEND_INSTANCE_ID="${BACKEND_INSTANCE_ID:-i-0eee461d227cab4aa}"
BACKEND_SECURITY_GROUP_ID="${BACKEND_SECURITY_GROUP_ID:-sg-0783dbd1fbd916b55}"
IMAGE_URI="${IMAGE_URI:?set IMAGE_URI to an ECR image URI}"
CERTIFICATE_ARN="${CERTIFICATE_ARN:-}"

VPC_ID="${VPC_ID:-$(
  AWS_PROFILE="$AWS_PROFILE" aws ec2 describe-vpcs \
    --region "$AWS_REGION" \
    --filters Name=isDefault,Values=true \
    --query 'Vpcs[0].VpcId' \
    --output text
)}"

SUBNET_IDS="${SUBNET_IDS:-$(
  AWS_PROFILE="$AWS_PROFILE" aws ec2 describe-subnets \
    --region "$AWS_REGION" \
    --filters Name=vpc-id,Values="$VPC_ID" Name=map-public-ip-on-launch,Values=true \
    --query 'Subnets[].SubnetId' \
    --output text | tr '\t' ','
)}"

PARAMETERS=(
  "ProjectName=${PROJECT_NAME}"
  "DomainName=${DOMAIN_NAME}"
  "ContainerImage=${IMAGE_URI}"
  "VpcId=${VPC_ID}"
  "PublicSubnets=${SUBNET_IDS}"
  "BackendInstanceId=${BACKEND_INSTANCE_ID}"
  "BackendSecurityGroupId=${BACKEND_SECURITY_GROUP_ID}"
  "Markets=${MARKETS}"
  "ExchangeServerUrl=${EXCHANGE_SERVER_URL}"
)

if [[ -n "$CERTIFICATE_ARN" ]]; then
  PARAMETERS+=("CertificateArn=${CERTIFICATE_ARN}")
fi

AWS_PROFILE="$AWS_PROFILE" aws cloudformation deploy \
  --region "$AWS_REGION" \
  --stack-name "$STACK_NAME" \
  --template-file infra/client-ecs/stack.yaml \
  --capabilities CAPABILITY_NAMED_IAM \
  --parameter-overrides "${PARAMETERS[@]}"

AWS_PROFILE="$AWS_PROFILE" aws cloudformation describe-stacks \
  --region "$AWS_REGION" \
  --stack-name "$STACK_NAME" \
  --query 'Stacks[0].Outputs' \
  --output table
