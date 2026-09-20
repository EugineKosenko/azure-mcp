# azure-mcp

A read-only [MCP](https://modelcontextprotocol.io) server for the parts of [Microsoft Azure](https://azure.microsoft.com)
that an assistant most often needs to look at and that the official Azure MCP server does not cover well:
actual costs, network resources, MySQL firewall rules, virtual machines and disks. It is written in Rust as a
literate [org-babel](https://orgmode.org/worg/org-contrib/babel/) program and talks the standard MCP `stdio`
transport, so an LLM assistant can call it directly.

This is an unofficial project. It is not affiliated with, endorsed by or sponsored by Microsoft Corporation.

No third-party MCP SDK crate is used: the `JSON-RPC 2.0` wire protocol is implemented directly on top of
`tokio` and `serde_json`, following the pattern of
[figma-mcp](https://github.com/EugineKosenko/figma-mcp) and
[org-tmetric-mcp](https://github.com/EugineKosenko/org-tmetric-mcp).

## Why

The official Azure MCP server exposes most of its services as a *hierarchical command router*: one tool
(`storage`, `compute`, `role`, ...) with a free-text `command` parameter inside. Permission rules of an MCP
client can only tell tools apart by name, so "allow reading, ask before writing" cannot be expressed for a
router tool. Beyond that, at the time of writing it has no namespace for network resources (public IPs,
network interfaces, security groups, private DNS), returns only retail SKU prices and Advisor
recommendations instead of actual costs, and its `mysql` tool has no firewall rules.

This server fills exactly those gaps, with **one tool per action** and **read-only** by design: there are no
write operations, so every tool can safely be allowed without a confirmation prompt. It does not replace the
official server; run both.

## Tools

Every tool takes an optional `group` (a resource group; without it the whole subscription is used) and
`sbscrptn` (a subscription id; without it `AZURE_SBSCRPTN`, then the default subscription of the Azure CLI).
Results are plain text, sorted by group and then by name.

- **costs** — actual costs (`ActualCost`) of a subscription or one resource group for a period, split by
  resource group, resource (shown as `group  type  name`), resource type, meter category or service, largest
  first, with a total. `period` is one of `MonthToDate`, `BillingMonthToDate`, `TheLastMonth`,
  `TheLastBillingMonth`, `WeekToDate`; or give `from` and `to` (`YYYY-MM-DD`, inclusive). With `daily` it
  prints the cost of every day and the average per complete day of each group. Cost Management lags by about
  half a day, so the last day of the data is always marked incomplete.
- **pubips** — public IP addresses: SKU, `Static` / `Dynamic`, the owner (network interface or NAT gateway)
  and the VM. An address that is not attached to anything is marked, because a static address without an
  owner is still billed. Optional `addr` keeps one address.
- **nics** — network interfaces: VM, security group, `enableIPForwarding`, and for every IP configuration the
  private address, virtual network / subnet and public IP.
- **nsgrules** — network security groups: the subnets and interfaces they are attached to, a summary of the
  ports open from any source, and the rules (custom and default) by direction and priority. `custom` hides
  the default rules.
- **firewall** — MySQL Flexible Servers: public network access, private endpoints and firewall rules. Each
  rule is annotated: `0.0.0.0` is "all Azure services", a private range is marked as such, and a public
  range is matched against the public IPs of the subscription (or reported as matching none). A server with
  no rules says what that means.
- **dnszones** — private DNS zones: virtual network links and A records.
- **vms** — virtual machines: size, OS, power state, private and public IPs.
- **disks** — managed disks: SKU, size, state and the VM they belong to. Unattached disks are marked.

## Authentication

The server holds no secrets. It gets an ARM access token by running

```sh
az account get-access-token --resource https://management.azure.com
```

so the [Azure CLI](https://learn.microsoft.com/cli/azure/) must be on the `PATH` of the process that starts
the server and must be signed in. The token is cached in memory until it expires. If the sign-in has expired
(for example `AADSTS50078`, an expired multi-factor authentication), tools answer with an error that says
what to run: `az login --scope https://management.core.windows.net//.default`. Only a person can do that
interactively.

Cost Management is rate limited per tenant and answers `429` often. A `429` is retried up to three times
with pauses of 5, 15 and 30 seconds (or the `Retry-After` the response carries, if it is not longer than a
minute); then the tool answers with a clear error. Cost responses are cached on disk for an hour, and a
cached answer says how old it is. Errors `401`, `403` and `404` come with a hint on what to check.

The account needs the built-in **Reader** role on what it should look at, and **Cost Management Reader** for
`costs`.

## Source layout

Every `.rs` file here is generated (tangled) from an `.org` file of the same purpose — edit the `.org`
source and re-tangle, never the `.rs` files directly. The prose of the `.org` files is in Ukrainian.

| org file | tangles to | purpose |
|---|---|---|
| `config.org` | `Cargo.toml` | dependencies |
| `env.org` | `.env.example` | environment variables |
| `gitignore.org` | `.gitignore` | ignored files |
| `main.org` | `src/main.rs` | the JSON-RPC/stdio protocol loop |
| `main-client.org` | `src/client.rs` | shared ARM client: token, retries, cache, paging |
| `main-tools.org` | `src/tools/mod.rs` | tool registry, dispatch, shared helpers |
| `main-tool-*.org` | `src/tools/*.rs` | one file per tool |

## Building

```sh
cargo build --release
```

## Configuration

Both variables are optional. Copy `.env.example` to `.env` for local `cargo run` testing only:

```
AZURE_SBSCRPTN=
#AZURE_CACHE=
```

`AZURE_SBSCRPTN` is the default subscription id (empty or unset: the default subscription of the Azure CLI).
`AZURE_CACHE` overrides the cost cache directory, which is `azure-mcp` in the system temporary directory by
default; leave it commented out unless you give it a value.

## Registering with Claude Code

```sh
claude mcp add --scope user azure-mcp -- /path/to/azure-mcp/target/release/azure-mcp
```

Add `-e AZURE_SBSCRPTN=<subscription id>` to pin a subscription. Since every tool is read-only they can all
be allowed without a prompt, e.g. `mcp__azure-mcp__costs`, `mcp__azure-mcp__pubips`, `mcp__azure-mcp__nics`,
`mcp__azure-mcp__nsgrules`, `mcp__azure-mcp__firewall`, `mcp__azure-mcp__dnszones`, `mcp__azure-mcp__vms`
and `mcp__azure-mcp__disks` in `permissions.allow`.

## License

[MIT](LICENSE)
