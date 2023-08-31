import { IVpc } from "aws-cdk-lib/aws-ec2";
import { IHostedZone } from "aws-cdk-lib/aws-route53";
import { ICertificate } from "aws-cdk-lib/aws-certificatemanager";


export type EcsParams = {
  vpc: IVpc;
  zone: IHostedZone;
  privateZone: IHostedZone;
  certificate?: ICertificate;
  defaultEnvironment: { [key: string]: string}
};
