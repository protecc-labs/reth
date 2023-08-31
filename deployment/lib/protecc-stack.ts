import { Stack, StackProps } from 'aws-cdk-lib';
import { Construct } from 'constructs';
import { Vpc } from 'aws-cdk-lib/aws-ec2';
import { HostedZone } from 'aws-cdk-lib/aws-route53';
import { Secret } from 'aws-cdk-lib/aws-secretsmanager';
import { DnsValidatedCertificate } from 'aws-cdk-lib/aws-certificatemanager';
import { StringParameter } from 'aws-cdk-lib/aws-ssm';

import { EcsResources } from './resources/ecs';

const {
  NODE_ENV = 'prod',
  VPC_ID,
  PRIVATE_SUBNET_IDS = '',
  PUBLIC_SUBNET_IDS = '',
} = process.env;

export class ProteccStack extends Stack {
  constructor(scope: Construct, id: string, props?: StackProps) {
    super(scope, id, props);

    const availabilityZones = this.availabilityZones;
    const vpcId = VPC_ID;

    const privateSubnetIds = JSON.parse(PRIVATE_SUBNET_IDS);
    const publicSubnetIds = JSON.parse(PUBLIC_SUBNET_IDS);

    const ZONE_ID = StringParameter.valueForStringParameter(
      this,
      `${NODE_ENV}ZoneId`
    );
    const ZONE_NAME = StringParameter.valueForStringParameter(
      this,
      `${NODE_ENV}ZoneName`
    );
    const TOP_DOMAIN = StringParameter.valueForStringParameter(
      this,
      `${NODE_ENV}TopDomain`
    );
    const PRIVATE_ZONE_ID = StringParameter.valueForStringParameter(
      this,
      `${NODE_ENV}PrivateZoneId`
    );
    const PRIVATE_ZONE_NAME = StringParameter.valueForStringParameter(
      this,
      `${NODE_ENV}PrivateZoneName`
    );
    const PRIVATE_TOP_DOMAIN = StringParameter.valueForStringParameter(
      this,
      `${NODE_ENV}PrivateTopDomain`
    );
    const PRIVATE_DB_DASHBOARD_SUBDOMAIN = StringParameter.valueForStringParameter(
      this,
      `${NODE_ENV}PrivateDBDashboardSubDomain`
    );
    const PRIVATE_DB_TRADING_SUBDOMAIN = StringParameter.valueForStringParameter(
      this,
      `${NODE_ENV}PrivateDBTradingSubDomain`
    );

    const zone = HostedZone.fromHostedZoneAttributes(
      this,
      'ProteccHostedZone',
      { hostedZoneId: ZONE_ID, zoneName: ZONE_NAME }
    );
    const privateZone = HostedZone.fromHostedZoneAttributes(
      this,
      'ProteccPrivateHostedZone',
      { hostedZoneId: PRIVATE_ZONE_ID, zoneName: PRIVATE_ZONE_NAME }
    );

    // const certificate = new DnsValidatedCertificate(
    //   this,
    //   `${NODE_ENV}ProteccSslCertificate`,
    //   {
    //     domainName: `*.${TOP_DOMAIN}`,
    //     subjectAlternativeNames: [TOP_DOMAIN],
    //     hostedZone: zone,
    //     region: this.region,
    //   }
    // );
    const vpc = Vpc.fromVpcAttributes(this, 'Vpc', {
      vpcId,
      availabilityZones,
      publicSubnetIds,
      privateSubnetIds,
    });

    const RDS_DB_HOST = StringParameter.valueForStringParameter(
      this,
      `${NODE_ENV}DashboardRdsDbHost`
    );
    const RDS_DB_PORT = StringParameter.valueForStringParameter(
      this,
      `${NODE_ENV}DashboardRdsDbPort`
    );
    const RDS_DB_USER = StringParameter.valueForStringParameter(
      this,
      `${NODE_ENV}RdsDbUser`
    );
    const RDS_DB_PASSWORD = Secret.fromSecretNameV2(
      this,
      `${NODE_ENV}PostgresPassword`,
      `${NODE_ENV}PostgresPassword`
    ).secretValue.unsafeUnwrap();

    const RDS_DB_DASHBOARD_NAME = `${NODE_ENV}DashboardProtecc`;
    const RDS_DB_TRADING_NAME = `${NODE_ENV}TradingProtecc`;
    const AWS_REGION = this.region;
    const DB_DASHBOARD_RDS_URL = `postgresql://${RDS_DB_USER}:${RDS_DB_PASSWORD}@${PRIVATE_DB_DASHBOARD_SUBDOMAIN}.${PRIVATE_TOP_DOMAIN}:${RDS_DB_PORT}/${RDS_DB_DASHBOARD_NAME}`;
    const DB_TRADING_RDS_URL = `postgresql://${RDS_DB_USER}:${RDS_DB_PASSWORD}@${PRIVATE_DB_TRADING_SUBDOMAIN}.${PRIVATE_TOP_DOMAIN}:${RDS_DB_PORT}/${RDS_DB_TRADING_NAME}`;

    const defaultEnvironment = {
      AWS_REGION,
      NODE_ENV,
      RDS_DB_HOST,
      RDS_DB_PORT,
      RDS_DB_USER,
      RDS_DB_PASSWORD,
      DB_DASHBOARD_RDS_URL,
      DB_TRADING_RDS_URL,
      TOP_DOMAIN,
      PRIVATE_TOP_DOMAIN,
      PRIVATE_DB_DASHBOARD_SUBDOMAIN,
      PRIVATE_DB_TRADING_SUBDOMAIN,
      BID_PERCENTAGE: '95',
      PRIVATE_KEY: '',
      CARGO_MANIFEST_DIRECTORY: '/home/',
    };

    new EcsResources(this, {
      zone,
      privateZone,
      // certificate,
      vpc,
      defaultEnvironment
    });
  }
}
