# Bugs

- `get_api_endpoint` preserves a trailing slash in `TACTICS_SERVER_URL`.
  A unit test with `TACTICS_SERVER_URL=https://example.test/` expected `https://example.test/api/v1/tactic`, but got `https://example.test//api/v1/tactic`.
