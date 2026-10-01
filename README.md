# azure-mcp

A read-only [MCP](https://modelcontextprotocol.io) server for the parts of [Microsoft Azure](https://azure.microsoft.com)
that an assistant most often needs to look at and that the official Azure MCP server does not cover well:
actual costs, retail storage prices, network resources, MySQL firewall rules, virtual machines, disks and
snapshots, storage accounts, images and galleries, Azure Backup, Azure Monitor metrics, Advisor recommendations, the subscription itself and its
regional quotas. It is written in Rust as
a literate [org-babel](https://orgmode.org/worg/org-contrib/babel/) program and talks the standard MCP
`stdio` transport, so an LLM assistant can call it directly.

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
network interfaces, security groups, private DNS), returns retail SKU prices for compute rather than storage
and no actual costs at all, and its `mysql` tool has no firewall rules. It also runs as a Node.js process
launched through `npx`, which was unreliable enough in practice (connection timeouts, dropped connections)
that duplicating its Advisor and pricing coverage as plain, direct ARM/REST calls turned out to be worth it.

This server fills exactly those gaps, with **one tool per action** and **read-only** by design: there are no
write operations, so every tool can safely be allowed without a confirmation prompt. It does not replace the
official server; run both.

## Tools

Most tools take an optional `group` (a resource group; without it the whole subscription is used) and
`sbscrptn` (a subscription id; without it `AZURE_SBSCRPTN`, then the default subscription of the Azure CLI).
`pricing` and `quota` take a `region` instead (they are not scoped to a resource group), and `pricing` needs
no `az` token at all — it calls the public, unauthenticated Azure Retail Prices API. Results are plain text,
sorted by group and then by name (or, for `quota`, by kind and name; `pricing` and `account` return a fixed
report, not a sortable list).

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
- **vms** — virtual machines: region, size, OS, power state, private and public IPs, boot security
  (`securityType`, e.g. `TrustedLaunch`, with `secureBoot` / `vTpm` flags when set) and creation time.
- **disks** — managed disks: SKU, size, generation (`hyperVGeneration`), security type, state and the VM they
  belong to. Unattached disks are marked — they are still billed. Generation and security type matter when
  reusing a disk (e.g. building an image from a snapshot): an unsupported combination is a common reason
  `az disk create --source` / `az image create` fails.
- **snapshots** — disk snapshots: SKU, size, generation, security type, OS, the source disk and whether the
  snapshot is full or incremental.
- **vnets** — virtual networks and their subnets: address space, subnet address prefix and the network
  security group attached to each subnet, one line per subnet. Replaces listing the subnets of a network and
  then checking each one's NSG separately.
- **resources** — every resource of every type in a group or subscription: name, group, type, region. Useful
  for an overview when no narrower tool fits.
- **pricing** — Standard HDD Managed Disk storage prices for a region, from the public Azure Retail Prices
  API: snapshot price per GB actually used, and the fixed monthly price of each disk tier (`S4`…`S80`, by
  provisioned size rounded up). Azure does not document which of the two models a standalone image
  (`Microsoft.Compute/images`) is billed under, so the tool returns both and suggests checking the actual
  cost with `costs` once a real image exists. With optional `size` (a VM size, e.g. `Standard_D4als_v6`) it
  also returns that size's on-demand hourly price, Linux and Windows separately (Windows costs more: it
  includes the license). `region` is required (e.g. `eastus`).
- **advisor** — Azure Advisor recommendations for the subscription (all categories: `Cost`,
  `HighAvailability`, `Security`, `Performance`, `OperationalExcellence`), each with its impact, the affected
  resource (or "subscription" for a subscription-level recommendation), the estimated annual saving when the
  recommendation carries one, and a short description. Calls `Microsoft.Advisor/recommendations` directly.
  Optional `category` and `group` narrow the result (`group` is filtered client-side: the API itself only
  supports subscription scope).
- **account** — the subscription itself: display name, id, state, tenant id, offer type (`quotaId`) and
  spending limit.
- **quota** — regional quota usage: `compute` (VM family vCPU quotas) and/or `network` (public IPs, VNets,
  NSGs, ...), each with its current and maximum value; a quota at its limit is marked. `region` is required;
  optional `kind` (`compute` or `network`) narrows to one of the two.
- **skus** — VM sizes available in a region: family (matches the family names `quota` reports), vCPU count
  and memory. Calls `Microsoft.Compute/skus` directly; `az vm list-skus` itself turned out to be
  resource-heavy (it installs and runs a CLI extension), which a plain REST call avoids. `region` is
  required; optional `size` keeps one VM size and `family` keeps the families whose name contains a text
  (without a filter the whole regional catalog is returned, about a hundred thousand characters). Each line
  ends with the disk-performance capabilities of the size: uncached IOPS and throughput (MiB/s), cached disk
  size, maximum data disks, Hyper-V generations, Premium IO, NVMe size and disk controller types.
- **support** — the subscription's support tickets: id, status, severity, service, problem classification,
  creation date and title, newest first. The submitter's contact details are in the API response but are
  deliberately not surfaced by this tool.
- **storage** — Storage accounts: name, group, region, SKU, kind, access tier and creation date. With
  `account` it adds the account's containers; with `container` the blobs of that container (name, size,
  type, tier, last modified) and their total size; with `sizes=true` the blob count and total size of every
  container (the ARM API does not report an account size). Read-only: account keys and blob contents are
  never used. Listing blobs is a data-plane call authorized with an Azure AD token, and needs the
  **Storage Blob Data Reader** role (see below).
- **images** — classic images (`Microsoft.Compute/images`): name, group, region, OS type, Hyper-V generation,
  OS state, OS disk size, creation date, source (`vm=`, `disk=`, `snapshot=` or `vhd=`) and state. Optional
  `name` keeps one image.
- **galleries** — Azure Compute Galleries: each gallery with its image definitions (OS, generation, OS state,
  `publisher/offer/sku`) and their versions (OS disk size, published and end-of-life dates, exclusion from
  `latest`, replication per region with replica count and storage type, source, state). Optional `name` keeps
  one gallery.
- **backup** — Azure Backup: Recovery Services vaults (group, region, SKU, storage redundancy LRS/GRS/ZRS,
  protected item count, immutability, soft delete, multi-user authorization and cross-region restore state), their policies (schedule, retention per daily/weekly/monthly/yearly level, instant
  snapshot retention, number of items using the policy) and protected items (name, type, policy, protection
  state, health, last backup result and time, recovery point count). Read-only: no restore operations and no
  protection changes. With `item` it lists the recovery points of one protected item instead (point id for
  `az backup restore`, time, consistency type, tier `InstantRP` / `HardenedRP`). Stored data size and per-item cost are not shown (ARM does not report them as a field;
  cost is available through `costs`). Optional `name` keeps one vault.
- **metrics** — Azure Monitor metrics of a resource (VM, disk, MySQL, storage account, ...) over a period: for
  each metric and aggregation a summary (average, maximum, minimum, interval count and, with `threshold`,
  how many intervals are strictly above it — for CPU with `interval=PT1H` that is the number of hours above
  the threshold) and, with `points=true`, the series of points. With `list=true` it lists the metrics the
  resource offers. Arguments: `timespan` (`30m`, `24h`, `7d` back from now) or an explicit `start` / `end`
  (ISO 8601 UTC); an unknown argument is an error, not silently ignored. The resource is a full `resource` id or a `name` (narrowed by `group` and `type`; several
  matches are listed instead of guessed). Read-only (`Reader` / `Monitoring Reader`); Azure keeps metrics for
  93 days.

## Authentication

`pricing` is the one exception: it calls `prices.azure.com`, a public API that needs no token and no Azure
CLI sign-in at all. Every other tool holds no secrets either — it gets an ARM access token by running

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

The blob listing of `storage` uses a second token, for the audience `https://storage.azure.com/`, obtained
the same way (`az account get-access-token --resource https://storage.azure.com/`); it is cached separately.

The account needs the built-in **Reader** role on what it should look at, **Cost Management Reader** for
`costs`, and **Storage Blob Data Reader** on a storage account or container for the blob listing of `storage`
(the management **Reader** role does not grant data access; without it Azure answers `403`).

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
be allowed without a prompt: `mcp__azure-mcp__costs`, `mcp__azure-mcp__pubips`, `mcp__azure-mcp__nics`,
`mcp__azure-mcp__nsgrules`, `mcp__azure-mcp__firewall`, `mcp__azure-mcp__dnszones`, `mcp__azure-mcp__vms`,
`mcp__azure-mcp__disks`, `mcp__azure-mcp__snapshots`, `mcp__azure-mcp__vnets`, `mcp__azure-mcp__resources`,
`mcp__azure-mcp__pricing`, `mcp__azure-mcp__advisor`, `mcp__azure-mcp__account`, `mcp__azure-mcp__quota`,
`mcp__azure-mcp__skus`, `mcp__azure-mcp__support`, `mcp__azure-mcp__storage`, `mcp__azure-mcp__images` and
`mcp__azure-mcp__galleries` and `mcp__azure-mcp__backup` and `mcp__azure-mcp__metrics` in `permissions.allow`.

## License

[MIT](LICENSE)
