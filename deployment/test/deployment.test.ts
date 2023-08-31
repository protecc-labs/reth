import * as cdk from "aws-cdk-lib";
import * as Deployment from "../lib/protecc-stack";

test("Lambda Create", () => {
  const app = new cdk.App();
  const stack = new Deployment.ProteccStack(app, "MyTestStack");
});
