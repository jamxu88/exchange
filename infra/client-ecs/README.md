# ECS Client Deployment

This directory contains the minimal AWS assets for deploying the Next.js client to ECS/Fargate behind an ALB, while forwarding backend exchange paths to the existing EC2 instance.

Current intended hostname:

- `exchange.jamesxu.dev`

Current routing split at the ALB:

- ECS client:
  - `/`
  - `/login`
  - `/trade`
  - `/admin`
  - `/api/auth/*`
  - `/api/health`
- EC2 backend:
  - `/api/v1/*`
  - `/ws`
  - `/health`
  - `/docs*`
  - `/api-doc/*`

## Usage

Build and push the client image:

```bash
infra/client-ecs/build_and_push.sh
```

Request an ACM certificate and print the DNS validation record:

```bash
infra/client-ecs/request_certificate.sh exchange.jamesxu.dev
```

Deploy or update the stack:

```bash
IMAGE_URI=490004617163.dkr.ecr.us-east-2.amazonaws.com/exchange-client:TAG \
CERTIFICATE_ARN=arn:aws:acm:us-east-2:... \
infra/client-ecs/deploy_stack.sh
```

If the certificate is not issued yet, omit `CERTIFICATE_ARN` and the stack will deploy HTTP-only on port `80` for pre-cutover testing.
