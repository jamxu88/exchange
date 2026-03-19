#!/usr/bin/env bash
set -euo pipefail

AWS_PROFILE="${AWS_PROFILE:-personal}"
AWS_REGION="${AWS_REGION:-us-east-2}"
DOMAIN_NAME="${1:?usage: request_certificate.sh <domain-name>}"

CERT_ARN="$(
  AWS_PROFILE="$AWS_PROFILE" aws acm request-certificate \
    --region "$AWS_REGION" \
    --domain-name "$DOMAIN_NAME" \
    --validation-method DNS \
    --query CertificateArn \
    --output text
)"

for _ in $(seq 1 20); do
  RECORD_NAME="$(
    AWS_PROFILE="$AWS_PROFILE" aws acm describe-certificate \
      --region "$AWS_REGION" \
      --certificate-arn "$CERT_ARN" \
      --query 'Certificate.DomainValidationOptions[0].ResourceRecord.Name' \
      --output text 2>/dev/null || true
  )"
  if [[ -n "${RECORD_NAME}" && "${RECORD_NAME}" != "None" ]]; then
    break
  fi
  sleep 2
done

AWS_PROFILE="$AWS_PROFILE" aws acm describe-certificate \
  --region "$AWS_REGION" \
  --certificate-arn "$CERT_ARN" \
  --query 'Certificate.{CertificateArn:CertificateArn,Status:Status,RecordName:DomainValidationOptions[0].ResourceRecord.Name,RecordType:DomainValidationOptions[0].ResourceRecord.Type,RecordValue:DomainValidationOptions[0].ResourceRecord.Value}' \
  --output json
