Finally, raise any security concerns: does the change introduce new malicious attack
vectors? Call out untrusted input reaching a path, command, query, or ref; missing
bounds; a guard that fails open. Where inputs are validated, ask for table-driven
unit tests that prove the validation is safe — adversarial cases (traversal,
injection, overflow, oversize) that assert the input is rejected.
