import { Stack, Duration } from 'aws-cdk-lib';
import { ApplicationMultipleTargetGroupsFargateService } from 'aws-cdk-lib/aws-ecs-patterns';
import {
  Cluster,
  FargateTaskDefinition,
  ContainerImage,
  LogDriver,
} from 'aws-cdk-lib/aws-ecs';
import {
  ApplicationProtocol,
  SslPolicy,
  ApplicationTargetGroup,
  ApplicationLoadBalancer,
} from 'aws-cdk-lib/aws-elasticloadbalancingv2';
import { ARecord, RecordTarget, CnameRecord } from 'aws-cdk-lib/aws-route53';
import { LoadBalancerTarget } from 'aws-cdk-lib/aws-route53-targets';
import { Secret } from 'aws-cdk-lib/aws-secretsmanager';
import { PolicyDocument, Role, ServicePrincipal } from 'aws-cdk-lib/aws-iam';
import { SecurityGroup } from 'aws-cdk-lib/aws-ec2';
import { StringParameter } from 'aws-cdk-lib/aws-ssm';

import { EcsParams } from './../types';

const config = require('../../config.json');
const { NODE_ENV } = process.env;

const getEnvironments = (scope, environment) => {
  if (!environment) {
    return {};
  }
  return Object.entries(environment).reduce(
    (resultEnv, [envName, paramName]) => ({
      ...resultEnv,
      [envName]: Secret.fromSecretNameV2(
        scope,
        `${NODE_ENV}${paramName}`,
        `${NODE_ENV}${paramName}`
      ).secretValue.unsafeUnwrap(),
    }),
    {}
  );
};

export class EcsResources {
  constructor(scope: Stack, params: EcsParams) {
    const { zone, privateZone, vpc, certificate, defaultEnvironment } = params;

    const serviceSecurityGroup = SecurityGroup.fromSecurityGroupId(
      scope,
      `${NODE_ENV}SecurityGroupId`,
      StringParameter.valueForStringParameter(scope, `${NODE_ENV}SecurityGroupId`)
    );

    const ecsExecRole = new Role(scope, 'EcsExecWaifuRole', {
      roleName: `${NODE_ENV}EcsExecWaifuRole`,
      assumedBy: new ServicePrincipal('ecs-tasks.amazonaws.com'),
      inlinePolicies: {
        sessionAccessPolicy: PolicyDocument.fromJson({
          Version: '2012-10-17',
          Statement: [
            {
              Action: [
                'logs:CreateLogGroup',
                'logs:CreateLogStream',
                'logs:PutLogEvents',
                'ec2:CreateNetworkInterface',
                'ec2:DescribeNetworkInterfaces',
                'ec2:DeleteNetworkInterface',
                'ec2:AssignPrivateIpAddresses',
                'ec2:UnassignPrivateIpAddresses',
                'iam:ListRoles',
                'iam:ListOpenIdConnectProviders',
                'iam:GetRole',
                'iam:ListOpenIDConnectProviders',
                'iam:ListRoles',
                'iam:ListSAMLProviders',
                'iam:GetSAMLProvider',
                'lambda:GetPolicy',
                'lambda:ListFunctions',
                'lambda:InvokeFunction',
                'sqs:*',
                'sns:*',
                'ses:*',
                'mobiletargeting:GetApps',
                'acm:ListCertificates',
                'cloudfront:CreateInvalidation',
                'cognito-idp:*',
                'cognito-identity:*',
                'cognito-sync:*',
                's3:*',
              ],
              Resource: '*',
              Effect: 'Allow',
            },
            {
              Effect: 'Allow',
              Action: 'sts:AssumeRole',
              Resource: [`arn:aws:iam::${scope.account}:role/*`],
            },
          ],
        }),
      },
    });

    const cluster = new Cluster(
      scope,
      `${NODE_ENV}ProteccEcsCluster`,
      { clusterName: `${NODE_ENV}ProteccCluster`, vpc: vpc }
    );

    const serviceName = config.name;
    const fargateTaskDefinition = new FargateTaskDefinition(
      scope,
      `${NODE_ENV}Protecc${serviceName}TaskDef`,
      {
        memoryLimitMiB: config.memoryLimitMiB,
        cpu: config.cpu,
        executionRole: ecsExecRole,
        taskRole: ecsExecRole,
      }
    );

    fargateTaskDefinition.addContainer(
      `${NODE_ENV}Protecc${serviceName}Container`,
      {
        image: ContainerImage.fromAsset(`${__dirname}/../../../`),
        logging: LogDriver.awsLogs({
          streamPrefix: `containers/${NODE_ENV}Protecc${serviceName}Container`,
        }),
        environment: {
          ...defaultEnvironment,
          ...getEnvironments(scope, config.environment),
        },
      }
    );

    const fargatePattern = new ApplicationMultipleTargetGroupsFargateService(
      scope,
      `${NODE_ENV}Protecc${serviceName}Service`,
      {
        assignPublicIp: false,
        cluster,
        taskDefinition: fargateTaskDefinition,
        desiredCount: 1,
        enableExecuteCommand: true,
        loadBalancers: [
          {
            name: `${NODE_ENV}${serviceName}LbProtecc`,
            idleTimeout: Duration.seconds(120),
            domainName: `${serviceName.toLowerCase()}.${defaultEnvironment.TOP_DOMAIN}`,
            domainZone: zone,
            listeners: [
              {
                name: `${NODE_ENV}${serviceName}ListenerProtecc`,
                protocol: ApplicationProtocol.HTTPS,
                // certificate: certificate,
                // sslPolicy: SslPolicy.TLS12_EXT,
              },
            ],
          },
        ],
        targetGroups: [
          {
            containerPort: config.container.port,
            listener: `${NODE_ENV}${serviceName}ListenerProtecc`,
          },
        ],
      }
    );

    // Setup AutoScaling policy
    const scaling = fargatePattern.service.autoScaleTaskCount({
      maxCapacity: config.service.maxCapacity,
    });
    scaling.scaleOnCpuUtilization(`${NODE_ENV}CpuScaling${serviceName}`, {
      targetUtilizationPercent: config.service.targetUtilizationPercent,
      scaleInCooldown: Duration.seconds(config.service.scaleInCooldown),
      scaleOutCooldown: Duration.seconds(config.service.scaleOutCooldown),
    });

    fargatePattern.targetGroups.forEach(
      (targetGroup: ApplicationTargetGroup) => {
        targetGroup.configureHealthCheck({
          path: config.healthCheck.path,
          timeout: Duration.seconds(config.healthCheck.timeout),
          interval: Duration.seconds(config.healthCheck.interval),
          unhealthyThresholdCount: config.healthCheck.unhealthyThresholdCount,
          healthyThresholdCount: config.healthCheck.healthyThresholdCount,
          healthyHttpCodes: '200-299',
        });
        scaling.scaleOnRequestCount(
          `${NODE_ENV}ProteccRequestScaling${serviceName}`,
          {
            requestsPerTarget: config.scaling.requestsPerTarget,
            targetGroup,
          }
        );
      }
    );
    fargatePattern.loadBalancers.forEach(
      (loadbalancer: ApplicationLoadBalancer) => {
        // do we need public endpoint for reth?
        // new ARecord(scope, `${NODE_ENV}Project${serviceName}Endpoint`, {
        //   recordName: serviceName.toLowerCase(),
        //   target: RecordTarget.fromAlias(
        //     new LoadBalancerTarget(loadbalancer)
        //   ),
        //   zone,
        // });
        new CnameRecord(scope, `${NODE_ENV}Record${serviceName}Endpoint`, {
          recordName: serviceName.toLowerCase(),
          domainName: loadbalancer.loadBalancerDnsName,
          zone: privateZone,
        });
      }
    );
  }
}
