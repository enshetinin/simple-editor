# Security policy

SimpleEditor handles local files, HTTP requests and secrets (variables kept
in memory or in the OS credential store). Please report vulnerabilities
privately rather than in public issues.

**Reporting channel: pending decision by the project owner.** Until it is
defined here, use GitHub's private vulnerability reporting for this
repository if it is enabled, and do not include working exploits for
other people's systems.

Design commitments we treat as security bugs if broken:
- cURL text, URLs and requests are never executed by a shell.
- TLS certificate verification is never disabled.
- `Authorization` and `Cookie` are not sent to another origin on redirect.
- Secret values are never written to project files, history, exports or
  logs, and never fall back to plain text when the OS store fails.
- Saving never replaces a file with a partial one.
