# Moonlight / Sunshine Pairing Research

Date: 2026-08-11

## Scope classification

`docs-only`

This note records the current Moonlight Qt and Sunshine pairing flow from
first-party source. It is evidence for AUV pairing design, not approval to add
a browser transport or replace AUV's current pairing API.

## Conclusion

Sunshine does not create the first token. Moonlight creates a random four-digit
PIN, displays it, and starts a pending pairing request. The host owner enters
that PIN and a client name in Sunshine's authenticated Web UI. The PIN is only
a short-lived trust bootstrap: successful pairing leaves Moonlight with its
long-lived client private key and a pinned Sunshine certificate, and leaves
Sunshine with the trusted Moonlight client certificate.

The useful model for AUV is therefore:

```text
controller creates and displays a short code
  -> controller submits its public identity and waits
  -> host owner enters the code to approve that waiting request
  -> both sides prove possession of their private keys
  -> both sides retain long-lived public-key identities
```

This resolves the bootstrap question without requiring a pre-authorized JS
caller to create a server-side token. AUV should borrow the interaction model,
not GameStream's exact four-digit, plaintext-HTTP, AES-ECB construction.

## Discovery and PIN origin

Moonlight browses `_nvstream._tcp.local.` over mDNS and also permits manual host
entry; it initially reads `/serverinfo` over HTTP because it has no pinned host
certificate yet. Its default GameStream ports are HTTP 47989 and HTTPS 47984
([Moonlight discovery](https://github.com/moonlight-stream/moonlight-qt/blob/2e13ed9977bc31c73caf8428f08f58d793313ece/app/backend/computermanager.cpp#L367-L382),
[initial server information](https://github.com/moonlight-stream/moonlight-qt/blob/2e13ed9977bc31c73caf8428f08f58d793313ece/app/backend/computermanager.cpp#L844-L871),
[default ports](https://github.com/moonlight-stream/moonlight-qt/blob/2e13ed9977bc31c73caf8428f08f58d793313ece/app/backend/nvaddress.h#L5-L6)).
Sunshine advertises the same mDNS service
([Sunshine service constants](https://github.com/LizardByte/Sunshine/blob/3cba9baebac882b336be3ebe129ee612cb189853/src/platform/common.h#L1243-L1244)).

When pairing starts, Moonlight generates a random value modulo 10,000 and
formats it as four digits. It displays the PIN and immediately begins the
pairing task
([PIN generation](https://github.com/moonlight-stream/moonlight-qt/blob/2e13ed9977bc31c73caf8428f08f58d793313ece/app/backend/computermanager.cpp#L1006-L1009),
[UI initiation](https://github.com/moonlight-stream/moonlight-qt/blob/2e13ed9977bc31c73caf8428f08f58d793313ece/app/gui/PcView.qml#L235-L243)).
The owner copies this client-generated PIN into Sunshine's PIN page, as the
[Sunshine setup guide](https://github.com/LizardByte/Sunshine/blob/3cba9baebac882b336be3ebe129ee612cb189853/docs/getting_started.md)
describes.

## Exact pairing sequence

Moonlight already has a persistent self-signed RSA-2048 client certificate,
private key, and unique ID. It creates these locally on first use and stores
them in platform `QSettings`
([Moonlight identity creation and loading](https://github.com/moonlight-stream/moonlight-qt/blob/2e13ed9977bc31c73caf8428f08f58d793313ece/app/backend/identitymanager.cpp#L29-L151)).

The GameStream exchange is five `GET /pair` calls. The first four use HTTP; the
last call uses HTTPS with both certificates:

1. **Offer identity and wait for approval.** Moonlight generates a 16-byte
   salt. For Sunshine and other generation-7-or-newer hosts it derives an
   AES-128 key from the first 16 bytes of
   `SHA-256(salt || UTF-8 PIN)`, then sends
   `phrase=getservercert`, the salt, its unique ID, and its PEM client
   certificate. Sunshine creates an in-memory session keyed by the unique ID
   and parks the HTTP response while it waits for the host owner
   ([Moonlight stage 1](https://github.com/moonlight-stream/moonlight-qt/blob/2e13ed9977bc31c73caf8428f08f58d793313ece/app/backend/nvpairingmanager.cpp#L207-L263),
   [Sunshine pending session](https://github.com/LizardByte/Sunshine/blob/3cba9baebac882b336be3ebe129ee612cb189853/src/nvhttp.cpp#L691-L743)).

2. **Host owner supplies the same PIN.** Sunshine's authenticated,
   CSRF-protected Web UI posts `{pin, name}` to `/api/pin`. Sunshine requires
   four decimal digits, derives the same AES key, attaches the friendly name,
   and completes the parked response with its PEM certificate
   ([Web API](https://github.com/LizardByte/Sunshine/blob/3cba9baebac882b336be3ebe129ee612cb189853/src/confighttp.cpp#L1334-L1382),
   [PIN validation and response](https://github.com/LizardByte/Sunshine/blob/3cba9baebac882b336be3ebe129ee612cb189853/src/nvhttp.cpp#L774-L820),
   [key derivation](https://github.com/LizardByte/Sunshine/blob/3cba9baebac882b336be3ebe129ee612cb189853/src/crypto.cpp#L331-L353)).

3. **Sunshine proves knowledge of the PIN and its private key.** Moonlight
   provisionally pins the returned host certificate, creates a random 16-byte
   challenge, AES-ECB encrypts it, and sends `clientchallenge`. Sunshine
   decrypts it, creates a random 16-byte server secret and challenge, then
   returns encrypted
   `SHA-256(clientChallenge || serverCertSignature || serverSecret) || serverChallenge`
   ([Moonlight stage 2](https://github.com/moonlight-stream/moonlight-qt/blob/2e13ed9977bc31c73caf8428f08f58d793313ece/app/backend/nvpairingmanager.cpp#L265-L297),
   [Sunshine stage 2](https://github.com/LizardByte/Sunshine/blob/3cba9baebac882b336be3ebe129ee612cb189853/src/nvhttp.cpp#L471-L519)).

4. **Moonlight proves knowledge of the PIN; Sunshine proves its private key.**
   Moonlight creates a random client secret and sends the encrypted, 32-byte
   padded `SHA-256(serverChallenge || clientCertSignature || clientSecret)` as
   `serverchallengeresp`. Sunshine retains that decrypted hash and returns
   `serverSecret || RSA-SHA256-sign(serverSecret)`. Moonlight verifies the
   signature using the certificate from stage 1 and checks the earlier hash.
   Failure is classified as a possible MITM or an incorrect PIN
   ([Moonlight stage 3 and verification](https://github.com/moonlight-stream/moonlight-qt/blob/2e13ed9977bc31c73caf8428f08f58d793313ece/app/backend/nvpairingmanager.cpp#L298-L339),
   [Sunshine stage 3](https://github.com/LizardByte/Sunshine/blob/3cba9baebac882b336be3ebe129ee612cb189853/src/nvhttp.cpp#L521-L555)).

5. **Moonlight proves its private key and activates mTLS.** Moonlight sends
   `clientSecret || RSA-SHA256-sign(clientSecret)` as `clientpairingsecret`.
   Sunshine verifies that signature with the submitted client certificate and
   recomputes the retained client hash. Only then does it persist and trust the
   client certificate. Moonlight finally calls
   `https://host/pair?...phrase=pairchallenge` while presenting its client
   certificate and accepting only the provisionally pinned host certificate;
   success confirms that the new mutual-TLS identity works
   ([Moonlight stages 4 and 5](https://github.com/moonlight-stream/moonlight-qt/blob/2e13ed9977bc31c73caf8428f08f58d793313ece/app/backend/nvpairingmanager.cpp#L341-L371),
   [Sunshine verification and authorization](https://github.com/LizardByte/Sunshine/blob/3cba9baebac882b336be3ebe129ee612cb189853/src/nvhttp.cpp#L557-L613)).

Moonlight retains compatibility with legacy NVIDIA GameStream hosts older than
generation 7 by substituting SHA-1 for SHA-256. That is a client compatibility
branch, not the current Sunshine behavior
([algorithm selection](https://github.com/moonlight-stream/moonlight-qt/blob/2e13ed9977bc31c73caf8428f08f58d793313ece/app/backend/nvpairingmanager.cpp#L207-L228)).

## Persistence, later authentication, and revocation

Sunshine stores a server-assigned UUID, friendly name, PEM client certificate,
and enabled flag for each paired client in its state JSON. At startup or after
mutation it rebuilds its accepted certificate stores from this record
([Sunshine state model and loading](https://github.com/LizardByte/Sunshine/blob/3cba9baebac882b336be3ebe129ee612cb189853/src/nvhttp.cpp#L166-L180),
[state persistence](https://github.com/LizardByte/Sunshine/blob/3cba9baebac882b336be3ebe129ee612cb189853/src/nvhttp.cpp#L243-L392)).
Moonlight keeps its own certificate/private key globally and stores the pinned
Sunshine certificate with that host
([host certificate persistence](https://github.com/moonlight-stream/moonlight-qt/blob/2e13ed9977bc31c73caf8428f08f58d793313ece/app/backend/nvcomputer.cpp#L80-L94)).

On later HTTPS requests, Moonlight presents its client certificate and only
ignores TLS errors when the peer certificate exactly matches the pinned host
certificate
([Moonlight TLS pinning](https://github.com/moonlight-stream/moonlight-qt/blob/2e13ed9977bc31c73caf8428f08f58d793313ece/app/backend/nvhttp.cpp#L435-L455),
[client certificate attachment](https://github.com/moonlight-stream/moonlight-qt/blob/2e13ed9977bc31c73caf8428f08f58d793313ece/app/backend/nvhttp.cpp#L481-L504)).
Sunshine verifies the presented certificate against the paired-client stores
and checks that the matching client remains enabled
([Sunshine TLS client verification](https://github.com/LizardByte/Sunshine/blob/3cba9baebac882b336be3ebe129ee612cb189853/src/nvhttp.cpp#L1287-L1342)).
There is no bearer or refresh token in this path; the durable credential is the
client private key corresponding to the stored certificate.

Unpairing one client removes the record by Sunshine's UUID, saves state, and
reloads the certificate stores immediately. Unpair-all clears both persistent
records and the in-memory trust stores
([Sunshine revocation](https://github.com/LizardByte/Sunshine/blob/3cba9baebac882b336be3ebe129ee612cb189853/src/nvhttp.cpp#L1417-L1438)).
Moonlight may still retain its old host entry and pinned host certificate, but
its client certificate is no longer trusted by Sunshine; it must pair again.

## Security boundary

The four-digit PIN has only 10,000 possibilities, and the cryptographic
pairing phases before final mTLS travel over HTTP. Sunshine's official 2025
advisory states that an on-path observer could capture enough traffic to brute
force the PIN offline; older releases also permitted replayed or out-of-order
phases that could substitute an attacker's certificate. The fix released in
2025.118.151840 added state-machine ordering and session destruction on errors
([GHSA-3hrw-xv8h-9499](https://github.com/LizardByte/Sunshine/security/advisories/GHSA-3hrw-xv8h-9499),
[current failure handling](https://github.com/LizardByte/Sunshine/blob/3cba9baebac882b336be3ebe129ee612cb189853/src/nvhttp.cpp#L426-L438)).

Sunshine also published a later critical advisory for incorrect client
certificate validation in releases before 2026.516.143833. Deployments should
run that version or newer
([GHSA-ph75-mgxh-mv57](https://github.com/LizardByte/Sunshine/security/advisories/GHSA-ph75-mgxh-mv57)).

For AUV, the transferable design is client-originated short-code approval plus
mutual proof of long-lived keys. A new design should use a modern PAKE or a
TLS-bound, single-use approval exchange rather than copying the legacy
GameStream primitives.
