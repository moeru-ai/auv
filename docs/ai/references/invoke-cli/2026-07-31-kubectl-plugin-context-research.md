# Kubectl Plugin Context Research

Date: 2026-07-31

Status: research note for the provisional AUV plugin invocation contract. This
note describes current upstream source behavior; it does not stabilize AUV API
names.

## Conclusion

Kubectl provides a useful executable discovery and process-replacement model,
but it does **not** inject its resolved cluster, context, namespace, or global
flags into plugins. A kubectl plugin is an independently implemented CLI:

- kubectl finds a `kubectl-*` executable on `PATH`;
- arguments after the matched plugin name are passed through unchanged;
- the existing process environment is relayed, with `KUBECTL_PATH` added;
- the plugin must declare and parse `--kubeconfig`, `--context`, `--namespace`,
  and other compatible flags itself;
- absent explicit flags, the plugin normally reloads kubeconfig through
  `client-go`/`cli-runtime`, thereby observing `KUBECONFIG`, the current context,
  and the context's default namespace.

Therefore the proposed AUV behavior

```text
auv --run <run-id> --device <name> netease-music search ...
  -> auv-netease-music search ...
  + normalized AUV run/device context in the child environment
```

is intentionally stronger and more reliable than kubectl's contract. It should
be documented as an AUV contract, not as behavior inherited from kubectl.

## Executable discovery and hierarchical names

Kubectl recognizes executable files beginning with `kubectl-` on `PATH`.
`DefaultPluginHandler.Lookup` uses `exec.LookPath`, so normal `PATH` order
decides which duplicate wins ([kubectl `plugin.go` lines 57-67](https://github.com/kubernetes/kubernetes/blob/8e608f6b90eaae92055474eeddbd92784c5830df/staging/src/k8s.io/kubectl/pkg/cmd/plugin.go#L57-L67)).
Built-in commands take precedence; plugin lookup is attempted only when Cobra
does not find a built-in command, with a narrow exception for allowed nested
commands such as `create`
([kubectl `cmd.go` lines 106-159](https://github.com/kubernetes/kubernetes/blob/8e608f6b90eaae92055474eeddbd92784c5830df/staging/src/k8s.io/kubectl/pkg/cmd/cmd.go#L106-L159)).

Discovery is hierarchical. Kubectl replaces dashes in command path components
with underscores, tries the longest executable name first, and progressively
shortens the candidate. Once a binary is found, unmatched path components are
forwarded as its argv
([kubectl `plugin.go` lines 107-155](https://github.com/kubernetes/kubernetes/blob/8e608f6b90eaae92055474eeddbd92784c5830df/staging/src/k8s.io/kubectl/pkg/cmd/plugin.go#L107-L155)). For example, `kubectl foo bar baz`
tries `kubectl-foo-bar-baz`, then `kubectl-foo-bar`, then `kubectl-foo`; if the
last exists it receives `bar baz`.

Krew is a package manager/install layout for this same protocol, not the plugin
runtime. It creates a `kubectl-<plugin>` symlink to the installed entrypoint and
converts plugin-name dashes to underscores
([Krew plugin manifest lines 183-207](https://github.com/kubernetes-sigs/krew/blob/299f8e0d1e917eec36fdd665b7435d4830001e60/site/content/docs/developer-guide/plugin-manifest.md#L183-L207)).

Implication for AUV: `auv netease-music <subcommand> ...` mapping to
`auv-netease-music <subcommand> ...` is consistent with the useful part of the
kubectl model. AUV should likewise reserve built-in names and define duplicate
resolution explicitly. It need not copy kubectl's dash-to-underscore quirk
unless compatibility requires it.

## Global flags: consumed, forwarded, and inherited

The common assumption that kubectl parses global flags and injects their
resolved values into plugins is incorrect.

`HandlePluginCommand` stops executable-name matching at the first flag and
returns `flags cannot be placed before plugin name` when a flag is the first
remaining argument
([kubectl `plugin.go` lines 109-121](https://github.com/kubernetes/kubernetes/blob/8e608f6b90eaae92055474eeddbd92784c5830df/staging/src/k8s.io/kubectl/pkg/cmd/plugin.go#L109-L121)). Arguments after the matched plugin name are passed directly to the
plugin
([lines 123-155](https://github.com/kubernetes/kubernetes/blob/8e608f6b90eaae92055474eeddbd92784c5830df/staging/src/k8s.io/kubectl/pkg/cmd/plugin.go#L123-L155)). Consequently:

```text
kubectl tree pods --context dev     # tree receives and must parse the flag
kubectl --context dev tree pods     # not a supported plugin-context mechanism
```

Krew's own best-practices guide says plugins should support common options such
as `--namespace`, and recommends `genericclioptions` to add `--kubeconfig`,
`--context`, and other kubectl-shaped flags
([Krew best practices lines 26-42](https://github.com/kubernetes-sigs/krew/blob/299f8e0d1e917eec36fdd665b7435d4830001e60/site/content/docs/developer-guide/develop/best-practices.md#L26-L42)). That recommendation would be unnecessary if kubectl resolved and injected
those values.

The process environment is inherited wholesale. Current kubectl removes any
preexisting `KUBECTL_PATH` and adds the path of the kubectl executable; it does
not add resolved kubeconfig/context/namespace variables
([kubectl `plugin.go` lines 148-175](https://github.com/kubernetes/kubernetes/blob/8e608f6b90eaae92055474eeddbd92784c5830df/staging/src/k8s.io/kubectl/pkg/cmd/plugin.go#L148-L175)). `KUBECONFIG` is visible only because it was already in the parent
environment.

## How plugins load context and construct clients

The official Go convention is for each plugin to construct
`genericclioptions.ConfigFlags`, attach those flags to its own Cobra command,
and then derive a REST configuration. `ConfigFlags` includes kubeconfig,
context, namespace, server, TLS, credential, impersonation, and timeout
overrides
([cli-runtime `config_flags.go` lines 83-110](https://github.com/kubernetes/cli-runtime/blob/c6b14e7f9cb18d23d75accaa9b0cfed0dfe3d355/pkg/genericclioptions/config_flags.go#L83-L110)); `AddFlags` exposes them on the plugin
([lines 371-442](https://github.com/kubernetes/cli-runtime/blob/c6b14e7f9cb18d23d75accaa9b0cfed0dfe3d355/pkg/genericclioptions/config_flags.go#L371-L442)).

`ToRESTConfig` builds the connection from kubeconfig loading rules plus flag
overrides
([cli-runtime lines 136-159](https://github.com/kubernetes/cli-runtime/blob/c6b14e7f9cb18d23d75accaa9b0cfed0dfe3d355/pkg/genericclioptions/config_flags.go#L136-L159)); context and namespace flags become explicit config overrides
([lines 234-258](https://github.com/kubernetes/cli-runtime/blob/c6b14e7f9cb18d23d75accaa9b0cfed0dfe3d355/pkg/genericclioptions/config_flags.go#L234-L258)). The underlying `client-go` default loader reads a path list from
`KUBECONFIG`, otherwise `~/.kube/config`
([client-go constants](https://github.com/kubernetes/kubernetes/blob/8e608f6b90eaae92055474eeddbd92784c5830df/staging/src/k8s.io/client-go/tools/clientcmd/loader.go#L40-L49),
[default loading rules](https://github.com/kubernetes/kubernetes/blob/8e608f6b90eaae92055474eeddbd92784c5830df/staging/src/k8s.io/client-go/tools/clientcmd/loader.go#L158-L180)).

Concrete implementations demonstrate two normal client styles:

- The official sample plugin creates its own `ConfigFlags`, adds the flags to
  its command, and independently loads raw kubeconfig
  ([sample-cli-plugin `ns.go` lines 67-114](https://github.com/kubernetes/sample-cli-plugin/blob/91817e142ac230c0212d77c22a6b0a03b373719e/pkg/cmd/ns.go#L67-L114)).
- `kubectl-tree` derives one REST config, constructs a dynamic client plus a
  discovery client, and independently resolves the effective namespace from
  the config loader
  ([kubectl-tree `rootcmd.go` lines 116-152](https://github.com/ahmetb/kubectl-tree/blob/552f01639c77680fa21f907554fe9aefc23fc6bd/cmd/kubectl-tree/rootcmd.go#L116-L152)); it installs `ConfigFlags` on its own root command
  ([lines 220-246](https://github.com/ahmetb/kubectl-tree/blob/552f01639c77680fa21f907554fe9aefc23fc6bd/cmd/kubectl-tree/rootcmd.go#L220-L246)).
- `kubens` demonstrates the typed-client path: construct a `rest.Config` from
  kubeconfig and pass it to `kubernetes.NewForConfig`
  ([kubectx `list.go` lines 112-142](https://github.com/ahmetb/kubectx/blob/12ad6fb22e8c546ee2b54e7de38aa51c906832f7/cmd/kubens/list.go#L112-L142)).

For AUV, `AuvClient::from_envs()` is analogous to `ConfigFlags` plus
`ToRESTConfig`, but should receive a canonical parent-resolved selection rather
than requiring every plugin to reproduce ambiguous device-name resolution.
Plugins may still expose their own `--device`/`--device-id` flags, but those are
plugin-owned overrides, not the only way to inherit AUV context.

## Process, streams, exit, and cancellation

On Unix, kubectl uses `syscall.Exec`: the plugin replaces kubectl, receives the
inherited environment, and naturally retains stdin/stdout/stderr and Unix signal
behavior. Its exit status is the command's exit status. On Windows, kubectl
starts a child, explicitly connects all three standard streams, waits, and
returns its error
([kubectl `plugin.go` lines 69-88](https://github.com/kubernetes/kubernetes/blob/8e608f6b90eaae92055474eeddbd92784c5830df/staging/src/k8s.io/kubectl/pkg/cmd/plugin.go#L69-L88)). There is no general plugin RPC, output envelope, or cancellation protocol.

AUV should preserve this composability:

- plugin stdout remains data/output and stderr remains diagnostics;
- successful and failing exit statuses propagate;
- stdin and TTY ownership pass through;
- on platforms where process replacement is unavailable, AUV explicitly
  forwards interruption/termination and waits for or terminates the child.

If AUV needs structured run recording, it should use the injected run context
and daemon/client API rather than capturing and reinterpreting arbitrary plugin
stdout as a protocol.

## Recommended AUV environment contract

The environment should carry normalized selection and discovery, while argv
after the plugin name remains wholly plugin-owned. The root should inject one
small inline JSON context plus its own executable path:

```text
AUV_CONTEXT={"device_id":"device_01H...","run_id":"run_01H...","daemon_endpoint":"unix:///...","config_profile":"default","credential_profile":"paired-mac-mini"}
AUV_PATH=/absolute/path/to/auv
```

Suggested parent resolution:

```text
explicit parent flag
  > existing inherited AUV invocation context
  > selected AUV config context
  > implicit local device / implicit run policy
```

`AuvClient::from_env()` should parse `AUV_CONTEXT`, prefer the canonical Device
ID, verify that an accompanying name still matches when both are present,
connect through the configured daemon/client layer, and attach the optional Run
ID to subsequent calls. A plugin's own flags may override these values only if
that plugin deliberately supports the override.

The root injects JSON through the process API, not shell interpolation. The
context carries short non-secret references and must not grow into a descriptor
or configuration payload. Parsers ignore unknown fields and apply normal
configuration/default resolution for absent optional fields, so the contract
does not require an independent version variable. Passing the context inline
also avoids context-file permission, cleanup, inheritance, and TOCTOU rules.

`AUV_PATH` has no context-selection role. It identifies the root executable
that launched the plugin so the plugin can invoke the same AUV installation;
plugins that do not need to re-enter the root CLI may ignore it.

## Security consequences

Kubectl's mechanism executes the first matching binary on `PATH`; its own plugin
listing code also reports shadowed duplicates
([kubectl plugin listing](https://github.com/kubernetes/kubernetes/blob/8e608f6b90eaae92055474eeddbd92784c5830df/staging/src/k8s.io/kubectl/pkg/cmd/plugin/plugin.go#L164-L197),
[shadow diagnostics](https://github.com/kubernetes/kubernetes/blob/8e608f6b90eaae92055474eeddbd92784c5830df/staging/src/k8s.io/kubectl/pkg/cmd/plugin/plugin.go#L230-L243)). An AUV plugin must therefore be treated as local code with the user's
authority, not as an untrusted RPC peer merely because daemon calls are
authenticated later.

In particular:

- do not inject bearer tokens, private keys, or reusable device credentials in
  environment variables;
- inject identity and endpoint references, then let the normal AUV client and
  daemon perform authentication/authorization;
- resolve executable paths deterministically, diagnose shadowing, and reserve
  built-in command names;
- make authorization depend on the authenticated caller and typed
  RPC, not on plugin executable name or claimed labels;
- avoid logging the entire child environment in run traces or error reports.

This preserves the proposed boundary: the AUV parent selects device/run
context; `AuvClient::from_envs()` consumes it; the daemon remains the authority
for access to local/remote runner services.
