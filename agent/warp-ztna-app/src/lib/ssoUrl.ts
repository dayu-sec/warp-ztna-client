export function ssoSignInUrl(verificationUriComplete: string, verificationUri: string): string {
  const complete = verificationUriComplete.trim();
  return complete !== "" ? complete : verificationUri.trim();
}
