#!/usr/bin/env bash
# Create (or destroy) the AWS Frankfurt fallback box for the hot role from the command line.
#
#   bash deploy/aws-create-fra1.sh            # create: key pair, security group, instance, Elastic IP; prints the ssh line
#   bash deploy/aws-create-fra1.sh --status   # show the instance, its IP and the ssh line
#   bash deploy/aws-create-fra1.sh --destroy  # terminate the instance and release its Elastic IP (asks first; YES=1 skips)
#
# Run it on the machine you will ssh from (your laptop): it opens port 22 to that machine's public IP and
# prints an ssh line that uses a key file on that machine. Run elsewhere (CloudShell, ohio1), set
# SSH_FROM=<laptop IP>/32 (curl https://checkip.amazonaws.com on the laptop) and copy the key file over.
#
# Needs: AWS CLI v2 with credentials that can manage EC2 in the region (`aws configure` or env vars), curl.
# This is the docs/03 section 4 / deploy/frankfurt-checklist.md "AWS eu-central-1" fallback: a c7i.2xlarge
# in a public subnet of the default VPC, no NAT, SSH only from your current IP, an Elastic IP so the
# address never changes, Ubuntu 24.04, 200 GB gp3. Expect 0.5-2 ms to the Frankfurt senders instead of
# the 0.1 ms of bare metal in the TeraSwitch building; accept below ~2.5 ms (runbook section 2).
#
# Everything is idempotent: a second run reuses the key pair, security group, instance and Elastic IP by name.
set -euo pipefail

REGION="${REGION:-eu-central-1}"
NAME="${NAME:-rugsnipe-fra1}"                 # instance Name tag; also the security group and key pair name
INSTANCE_TYPE="${INSTANCE_TYPE:-c7i.2xlarge}" # or c7i.4xlarge; c8g.* (Graviton) also works with install.sh
VOLUME_GB="${VOLUME_GB:-200}"
KEY_PEM="${KEY_PEM:-$HOME/.ssh/sniper-ohio.pem}" # reuse the Ohio key by importing its public half
SSH_FROM="${SSH_FROM:-}"                      # CIDR allowed to ssh; default = your current public IP /32

aws() { command aws --region "$REGION" --output text "$@"; }
log() { printf '%s %s\n' "$(date -u +%H:%M:%S)" "$*" >&2; }
die() { log "ERROR: $*"; exit 1; }

# `command -v aws` would find the aws() function above, so look for the binary itself
type -P aws >/dev/null || die "aws CLI not found (install AWS CLI v2 and run: aws configure)"
aws sts get-caller-identity >/dev/null || die "aws credentials missing or invalid (run: aws configure)"

find_instance() {
  aws ec2 describe-instances \
    --filters "Name=tag:Name,Values=$NAME" "Name=instance-state-name,Values=pending,running,stopping,stopped" \
    --query 'Reservations[].Instances[].InstanceId' | tr '\t' '\n' | grep -m1 . || true
}
local_key() { if [ -r "$KEY_PEM" ]; then echo "$KEY_PEM"; else echo "$HOME/.ssh/$NAME.pem"; fi; }

case "${1:-}" in
  --status)
    IID=$(find_instance); [ -n "$IID" ] || die "no instance named $NAME in $REGION"
    aws ec2 describe-instances --instance-ids "$IID" \
      --query 'Reservations[].Instances[].[InstanceId,State.Name,InstanceType,PublicIpAddress,Placement.AvailabilityZone]'
    IP=$(aws ec2 describe-instances --instance-ids "$IID" --query 'Reservations[0].Instances[0].PublicIpAddress')
    [ "$IP" = None ] || echo "ssh -i $(local_key) ubuntu@$IP"
    exit 0 ;;
  --destroy)
    IID=$(find_instance)
    # every Elastic IP this script allocated carries the Name tag (also catches one left unassociated by a crash)
    ALLOCS=$(aws ec2 describe-addresses --filters "Name=tag:Name,Values=$NAME" --query 'Addresses[].AllocationId' | tr '\t' '\n')
    [ -n "$IID" ] || [ -n "$ALLOCS" ] || die "no instance or Elastic IP named $NAME in $REGION"
    if [ "${YES:-}" != 1 ]; then
      ans=; read -r -p "terminate ${IID:-(no instance)} and release ${ALLOCS:-no Elastic IP} in $REGION? type yes: " ans || true
      [ "$ans" = yes ] || die "aborted"
    fi
    if [ -n "$IID" ]; then
      log "terminating $IID"
      aws ec2 terminate-instances --instance-ids "$IID" >/dev/null
      aws ec2 wait instance-terminated --instance-ids "$IID"
    fi
    for a in $ALLOCS; do log "releasing Elastic IP $a"; aws ec2 release-address --allocation-id "$a"; done
    log "done (the security group and key pair are kept)"
    exit 0 ;;
  "") ;;
  *) die "usage: $0 [--status|--destroy]" ;;
esac

# 0. where ssh will come from. checkip sees the machine running this script, which is wrong in CloudShell
#    or on another EC2 box (ohio1): there, insist on SSH_FROM so you do not lock your laptop out.
if [ -z "$SSH_FROM" ]; then
  if [ "${AWS_EXECUTION_ENV:-}" = CloudShell ] \
     || curl -fsS -m 1 -X PUT -H 'X-aws-ec2-metadata-token-ttl-seconds: 10' http://169.254.169.254/latest/api/token >/dev/null 2>&1; then
    die "this runs inside AWS (CloudShell/EC2), so its IP is not your laptop's. Run it on your laptop, or set SSH_FROM=<laptop IP>/32"
  fi
  SSH_FROM="$(curl -fsS https://checkip.amazonaws.com | tr -d '[:space:]')/32"
fi
case "$SSH_FROM" in */*) ;; *) SSH_FROM="$SSH_FROM/32" ;; esac

# 1. key pair: import the public half of your existing key so the same .pem works, else create a new one
if ! aws ec2 describe-key-pairs --key-names "$NAME" >/dev/null 2>&1; then
  [ -r "$KEY_PEM" ] || KEY_PEM="$HOME/.ssh/$NAME.pem"   # a key file left by an earlier run is reused too
  if [ -r "$KEY_PEM" ]; then
    PUB=$(mktemp); ssh-keygen -y -f "$KEY_PEM" > "$PUB"
    aws ec2 import-key-pair --key-name "$NAME" --public-key-material "fileb://$PUB" >/dev/null
    rm -f "$PUB"; log "key pair $NAME imported from $KEY_PEM"
  else
    mkdir -p "$HOME/.ssh"; chmod 700 "$HOME/.ssh"
    (umask 077; aws ec2 create-key-pair --key-name "$NAME" --key-type ed25519 --query KeyMaterial > "$KEY_PEM.tmp")
    mv "$KEY_PEM.tmp" "$KEY_PEM"; chmod 400 "$KEY_PEM"; log "key pair $NAME created: $KEY_PEM"
  fi
else
  log "key pair $NAME exists"
  KEY_PEM=$(local_key)
  [ -r "$KEY_PEM" ] || log "WARNING: no local private key for $NAME ($KEY_PEM); set KEY_PEM=... or delete the key pair and re-run"
fi

# 2. network: default VPC, a default subnet (they auto-assign public IPs) in an AZ that offers the type, no NAT
ARCH=$(aws ec2 describe-instance-types --instance-types "$INSTANCE_TYPE" \
  --query 'InstanceTypes[0].ProcessorInfo.SupportedArchitectures' 2>/dev/null) \
  || die "instance type $INSTANCE_TYPE does not exist in $REGION"
AZS=$(aws ec2 describe-instance-type-offerings --location-type availability-zone \
  --filters "Name=instance-type,Values=$INSTANCE_TYPE" --query 'InstanceTypeOfferings[].Location' | tr '\t' ',')
[ -n "$AZS" ] || die "$INSTANCE_TYPE is not offered in any $REGION availability zone"
VPC=$(aws ec2 describe-vpcs --filters Name=isDefault,Values=true --query 'Vpcs[0].VpcId')
[ -n "$VPC" ] && [ "$VPC" != None ] || die "no default VPC in $REGION (create one: aws ec2 create-default-vpc --region $REGION)"
SUBNET=$(aws ec2 describe-subnets --filters "Name=vpc-id,Values=$VPC" "Name=default-for-az,Values=true" \
  "Name=availability-zone,Values=$AZS" --query 'Subnets[0].SubnetId')
[ -n "$SUBNET" ] && [ "$SUBNET" != None ] || die "no default subnet in $VPC in an AZ offering $INSTANCE_TYPE ($AZS) (aws ec2 create-default-subnet --availability-zone ...)"

# 3. security group: ssh from your IP only; nothing else inbound
SG=$(aws ec2 describe-security-groups --filters "Name=group-name,Values=$NAME" "Name=vpc-id,Values=$VPC" \
  --query 'SecurityGroups[0].GroupId' 2>/dev/null || true)
if [ -z "$SG" ] || [ "$SG" = None ]; then
  SG=$(aws ec2 create-security-group --group-name "$NAME" --description "rugsnipe fra1: ssh only" --vpc-id "$VPC" --query GroupId)
  log "security group $SG created"
fi
if ERR=$(aws ec2 authorize-security-group-ingress --group-id "$SG" --protocol tcp --port 22 --cidr "$SSH_FROM" 2>&1 >/dev/null); then
  log "ssh allowed from $SSH_FROM"
elif [[ $ERR == *InvalidPermission.Duplicate* ]]; then
  log "ssh rule for $SSH_FROM already present"
else
  die "could not open port 22 for $SSH_FROM: $ERR"
fi

# 4. the instance: Ubuntu 24.04 (canonical's SSM parameter always points at the current AMI)
IID=$(find_instance)
if [ -z "$IID" ]; then
  case "$ARCH" in *arm64*) A=arm64 ;; *) A=amd64 ;; esac
  AMI=$(aws ssm get-parameter \
    --name "/aws/service/canonical/ubuntu/server/24.04/stable/current/$A/hvm/ebs-gp3/ami-id" \
    --query Parameter.Value)
  ROOT=$(aws ec2 describe-images --image-ids "$AMI" --query 'Images[0].RootDeviceName')   # /dev/sda1 on Canonical AMIs
  log "launching $INSTANCE_TYPE ($A) from $AMI in $SUBNET"
  IID=$(aws ec2 run-instances --image-id "$AMI" --instance-type "$INSTANCE_TYPE" --key-name "$NAME" \
    --security-group-ids "$SG" --subnet-id "$SUBNET" --associate-public-ip-address \
    --block-device-mappings "[{\"DeviceName\":\"$ROOT\",\"Ebs\":{\"VolumeSize\":$VOLUME_GB,\"VolumeType\":\"gp3\",\"DeleteOnTermination\":true}}]" \
    --metadata-options HttpTokens=required \
    --tag-specifications "ResourceType=instance,Tags=[{Key=Name,Value=$NAME},{Key=role,Value=hot}]" \
                         "ResourceType=volume,Tags=[{Key=Name,Value=$NAME}]" \
    --query 'Instances[0].InstanceId')
  log "instance $IID launched; waiting until running"
else
  STATE=$(aws ec2 describe-instances --instance-ids "$IID" --query 'Reservations[0].Instances[0].State.Name')
  log "instance $IID exists ($STATE)"
  if [ "$STATE" = stopping ]; then aws ec2 wait instance-stopped --instance-ids "$IID"; STATE=stopped; fi
  if [ "$STATE" = stopped ]; then log "starting $IID"; aws ec2 start-instances --instance-ids "$IID" >/dev/null; fi
fi
aws ec2 wait instance-running --instance-ids "$IID"

# 5. Elastic IP so the address never changes (provider allow-lists and WireGuard depend on it)
EIP=$(aws ec2 describe-addresses --filters "Name=instance-id,Values=$IID" --query 'Addresses[0].PublicIp' 2>/dev/null || true)
if [ -z "$EIP" ] || [ "$EIP" = None ]; then
  # reuse an unattached one from an interrupted run before allocating a new one
  ALLOC=$(aws ec2 describe-addresses --filters "Name=tag:Name,Values=$NAME" \
    --query 'Addresses[?AssociationId==null].AllocationId | [0]')
  if [ -z "$ALLOC" ] || [ "$ALLOC" = None ]; then
    ALLOC=$(aws ec2 allocate-address --domain vpc --tag-specifications "ResourceType=elastic-ip,Tags=[{Key=Name,Value=$NAME}]" --query AllocationId)
  fi
  aws ec2 associate-address --instance-id "$IID" --allocation-id "$ALLOC" >/dev/null
  EIP=$(aws ec2 describe-addresses --allocation-ids "$ALLOC" --query 'Addresses[0].PublicIp')
  log "Elastic IP $EIP attached"
fi
log "waiting for the status checks (2-5 min)"
aws ec2 wait instance-status-ok --instance-ids "$IID"

cat <<MSG

fra1 is up in $REGION: $IID  $INSTANCE_TYPE  $EIP

  ssh -i $KEY_PEM ubuntu@$EIP

Next (docs/08-runbook.md):
  section 2  acceptance test:  bash deploy/latency_probe.sh 30; ping the five Frankfurt targets (accept < ~2.5 ms on AWS)
  section 3  install:          git clone <repo> ~/rugsnipe && cd ~/rugsnipe && sudo bash deploy/install.sh --role hot
Later, to remove everything this script made:  bash deploy/aws-create-fra1.sh --destroy
MSG
