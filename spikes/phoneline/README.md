# Spike 1: PhoneLine calling probe (v0.5.7)

**Throwaway. Not for merge, not part of the tug build.** `npm run check`, `src-tauri` and CI don't
reference this directory, and its `Cargo.toml` declares its own empty `[workspace]`.

**The question:** can an unpackaged Win32 exe, given package identity by a *sparse* package
(signed with a self-signed cert, registered with `Add-AppxPackage -ExternalLocation`), pass the
restricted `phoneLineTransportManagement` check and dial through Dave's paired iPhone with
`Windows.ApplicationModel.Calls`? If yes, calling in tug is a go (ROADMAP.md, v0.5.7).

The probe prints each step and ends with one line:

- `RESULT: GO`: package identity present, `RequestAccessAsync` = Allowed, registered, connected,
  and a `PhoneLine` for the iPhone's transport with `CanDial = true`.
- `RESULT: NO-GO <why>`: says which step failed.

Flow: package identity → `PhoneLineTransportDevice` enumeration → `FromId` → `RequestAccessAsync` →
`IsRegistered` / `RegisterApp` / `IsRegistered` → `ConnectAsync` → `PhoneCallManager.RequestStoreAsync`
→ `PhoneLineWatcher` (lines until `EnumerationCompleted`, 15 s timeout) → with `--dial`:
`DialWithResultAsync` → `ChangeAudioDeviceAsync(RemoteDevice)`.

`RegisterApp` and `ConnectAsync` only run once access is Allowed, so a run without identity changes
nothing. Windows only lets the foreground app dial, and a console window belongs to the terminal,
not the probe. So the probe opens a small always-on-top **"tug call spike"** window while it runs and
closes it by itself. Leave it in front; if it says it isn't foreground, click it.

## Before you start

- Spike 0 conditions still hold: Handsfree Telephony is ticked for the iPhone in Devices and
  Printers, the iPhone is connected over Bluetooth, and Phone Link isn't installed (only one app can
  own a phone line).
- Use **Windows PowerShell** (`powershell.exe`) for the two scripts. Its PKI and Appx modules load
  natively, and `-ExecutionPolicy Bypass` applies to that one process only; your policy stays as it is.

## Steps for Dave, in order

### 0. Get the branch (separate folder, leaves your main checkout alone)

```powershell
cd C:\Users\jamesd\Github\tug
git fetch origin
git worktree add --detach ..\tug-spike origin/spike/phoneline
cd ..\tug-spike\spikes\phoneline
```

All later steps run from that `spikes\phoneline` folder.

### 1. Build

```powershell
cargo build --release
```

### 2. Baseline run, no identity yet

```powershell
.\target\release\tug-call-spike.exe
```

Expect `no identity (HRESULT 0x80073d54 ...)`, the iPhone found, `RequestAccessAsync` →
`DeniedBySystem`, and `RESULT: NO-GO access DeniedBySystem; ...`. That's the baseline: without
identity Windows refuses the restricted capability.

### 3. Create the dev signing certificate (changes your user certificate store)

You run this yourself. It creates a self-signed code-signing cert **`CN=Dave James Dev`** in
`Cert:\CurrentUser\My` (with private key, valid 1 year), exports `TugCallSpike.cer`, and imports it
into `Cert:\CurrentUser\TrustedPeople`. No admin. Step 7 shows how to remove it.

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\make-cert.ps1
```

### 4. Pack, sign and register the sparse package

Packs `package\` (manifest + logos only) with `makeappx pack /nv`, signs it with the cert above
(`signtool sign /fd SHA256 /sha1 <thumbprint>`), and runs `Add-AppxPackage -Path out\TugCallSpike.msix
-ExternalLocation <full path to target\release>`. This registers the package `DaveJames.TugCallSpike`
for your user.

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\pack-and-register.ps1
```

If you rebuild the exe later you don't need to re-register; the package points at `target\release`.

### 5. Run again, with identity

```powershell
.\target\release\tug-call-spike.exe
```

Expect `identity: DaveJames.TugCallSpike_1.0.0.0_x64__...`, then `RequestAccessAsync` → `Allowed`,
`IsRegistered (after): true`, `connected: true`, a line with `CanDial: true` for the iPhone, and
`RESULT: GO`.

**If access is `DeniedBySystem` even with identity:** open Settings › Privacy & security › Phone
calls, turn on phone call access (and "Let apps make phone calls"), then run step 5 again. If
`tug call spike` is listed there, make sure it's on.

### 6. Optional: place a real call

Only after step 5 says GO. Dial something harmless, such as your own voicemail or a number you
control. The call goes out on the iPhone. `--audio remote` (the default) asks Windows to keep call
audio on the phone. Hang up on the phone.

```powershell
.\target\release\tug-call-spike.exe --dial <voicemail-or-safe-number>
```

Watch for `DialCallStatus: Succeeded`, `call object returned`, and
`ChangeAudioDeviceAsync(RemoteDevice (the phone)): Succeeded`. To try PC audio instead, add
`--audio local`.

### 7. Cleanup (undo steps 3 and 4)

Unregister the sparse package:

```powershell
powershell.exe -NoProfile -Command "Get-AppxPackage DaveJames.TugCallSpike | Remove-AppxPackage"
```

Remove the dev certificate from both stores (the `-DeleteKey` also deletes its private key):

```powershell
powershell.exe -NoProfile -Command "Get-ChildItem Cert:\CurrentUser\My | Where-Object Subject -eq 'CN=Dave James Dev' | Remove-Item -DeleteKey; Get-ChildItem Cert:\CurrentUser\TrustedPeople | Where-Object Subject -eq 'CN=Dave James Dev' | Remove-Item"
```

Remove the spike folder:

```powershell
cd C:\Users\jamesd\Github\tug
git worktree remove --force ..\tug-spike
```

## If something goes wrong

- **`Add-AppxPackage` fails with 0x800B0109 / "root certificate ... not trusted"**: the deployment
  service didn't accept `CurrentUser\TrustedPeople`. The fallback needs **admin** and changes the
  *machine* store, so it's your call. From an elevated Windows PowerShell in this folder:
  `Import-Certificate -FilePath .\TugCallSpike.cer -CertStoreLocation Cert:\LocalMachine\TrustedPeople`,
  then rerun step 4. To undo it later (elevated):
  `Get-ChildItem Cert:\LocalMachine\TrustedPeople | Where-Object Subject -eq 'CN=Dave James Dev' | Remove-Item`.
- **Step 5 still prints `no identity`**: the exe's embedded `<msix>` element (in
  `tug-call-spike.exe.manifest`) must match `package\AppxManifest.xml` exactly (publisher, name,
  `applicationId="spike"`), and the exe must run from the registered `target\release` folder.
- **`IsRegistered (after): false`**: another app owns the phone line (Phone Link or similar).
- **Lines appear but none for the iPhone transport**: the probe prints every line's
  `TransportDeviceId`. Copy the output into the PR.
- **Dial fails**: check the `foreground:` line. If it says NO, click the window and retry.

Paste the full console output of steps 2, 5 and (if run) 6 into the PR.

## Files

- `src/main.rs`: the probe.
- `tug-call-spike.exe.manifest`: embedded by `build.rs` (via `embed-manifest`). Carries the
  `<msix publisher="CN=Dave James Dev" packageName="DaveJames.TugCallSpike" applicationId="spike"/>`
  identity link.
- `package/AppxManifest.xml`: the sparse package manifest (`uap10:AllowExternalContent`,
  `phoneCall`, `runFullTrust`, `unvirtualizedResources`, `phoneLineTransportManagement`,
  `mediumIL` / `win32App`, `AppListEntry="none"`). Logos are copies of tug's icons.
- `make-cert.ps1`, `pack-and-register.ps1`: Dave-run scripts for steps 3 and 4.
