# Accounts

1. How do I create an account on the app? When I load up the app as a first timer, no views or buttons to create an account. We should have that and I should provide endpoints for it.

# Transactions

1. How do I import my transactions? I could not find any buttons to do that or fields to upload any data.

# Bugs

[CRITICAL] Failing UI
I am unable to test because the live data loading freezes the UI
To recreate, do the following:

###### 1. Set Anthropic API key

- Create your .env file and set the `FYNANCE_ANTHROPIC_API_KEY`
- Key is in whatsapp chat

###### 2. Build the backend CLI

- See the `fynance/backend/RUNNING.md` for more instructions if you wish, but this is enough:

```
$ cd backend && cargo build --release
```

###### 3. Create an account

- From the `fynance/backend` directory, run:

```
target/release/fynance account add \
  --id revolut \
  --name "Revolut" \
  --institution Revolut \
  --type savings \
  --currency GBP
```

###### 4. Import statement as CSV file

```
target/release/fynance import ~/Downloads/revolut_all_time_statement.csv --account revolut
```

###### 5. Check the UI for changes

This is where it fails in the UI. The debug logs show the API requests being sent and received as 200, so I think this is a UI issue.

# Security

1. CORS is currently `CorsLayer::permissive()` in `backend/src/server/mod.rs:114`, which sends `Access-Control-Allow-Origin: *`. Combined with the loopback binding, this means any website the user visits in the same browser could issue `fetch('http://127.0.0.1:7433/api/...')` and read the user's finance data. Not a blocker for self-hosted MVP — blast radius is limited — but worth tightening to `http://127.0.0.1:<dev-port>` and `http://localhost:<dev-port>` explicitly when we get a chance, or relying on same-origin once the frontend is served from the backend in production mode.
