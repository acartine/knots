# Lessons

- Treat explicit end-to-end authorization as sufficient. Do not ask the operator to reconfirm
  routine in-scope commands when the execution environment already grants access.
- Distinguish session-provided instruction blocks from on-disk files. Verify file contents before
  attributing an instruction to a path or line number.
