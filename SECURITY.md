# Security policy

Please report vulnerabilities privately through GitHub's **Security** tab
instead of opening a public issue.

## Release trust model

Only pushes to `probably-undefined/gh-ai-credit-pulse` on `main` can enter the
publish job. Pull requests—including pull requests from forks—run with a
read-only token and cannot access the release job's token or OIDC identity.
The privileged job consumes a GitHub-hosted artifact, attests it, and publishes
it without checking out or executing repository code.

Installers verify the complete release archive with SHA-256 and
`gh attestation verify --repo probably-undefined/gh-ai-credit-pulse`. Any
verification failure stops installation before extraction.
