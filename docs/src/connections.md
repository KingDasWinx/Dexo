# Connections, TLS, SSH, and keychain

Connections store host, port, database, user, and driver options in SQLite. The password lives in the native keychain behind an opaque `secret_ref`.

TLS verifies certificates by default. Custom CA and client certificates are supported. Disabling verification is explicit and shown as a persistent warning.

SSH tunnels verify known hosts. A new or changed host key requires confirmation and is never accepted automatically.

If the keychain is missing or locked, Dexo asks for the secret for the current session. It does not write a file vault.
