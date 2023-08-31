#!/usr/bin/env node
import * as cdk from "aws-cdk-lib";
import { ProteccStack } from "../lib/protecc-stack";

const {
  NODE_ENV = "prod",
  RELEASE_VERSION = ''
} = process.env;

const app = new cdk.App();
new ProteccStack(app, `${NODE_ENV}DeploymentProteccRethStackTag${RELEASE_VERSION}`);
