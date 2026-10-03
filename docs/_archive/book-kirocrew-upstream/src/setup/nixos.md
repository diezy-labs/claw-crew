# NixOS

ClawCrew ships a multi-instance NixOS module at
[`nix/module.nix`](https://github.com/clawcrew-labs/clawcrew/blob/master/nix/module.nix).
It runs one or more agents under systemd with hardening defaults appropriate
for an internet-facing process, modelled on `services.restic.backups`.

## The package

The upstream flake builds ClawCrew from source. With Nix's `nix-command` and
`flakes` experimental features enabled, check the CLI without installing a
system service:

```sh
nix run github:clawcrew-labs/clawcrew -- --version
nix run github:clawcrew-labs/clawcrew -- --help
```

The default package is the ClawCrew CLI; the development toolchain is exposed
separately through `nix develop`. Building the package can take time on a cold
cache. To inspect a local checkout:

```sh
nix build .#clawcrew
./result/bin/clawcrew --version
nix flake check
```

Running the CLI through `nix run` does not add it to your login shell's `PATH`.
For a persistent NixOS installation, Nixpkgs provides `pkgs.clawcrew`:

<div class="os-tabs-src">

#### nix

```nix
environment.systemPackages = [ pkgs.clawcrew ];
```

</div>

The Nixpkgs package version follows your Nixpkgs pin; the upstream flake follows
the selected ClawCrew revision. These can differ. The module below defaults to
`pkgs.clawcrew` and starts its `clawcrew daemon` command. Set
`services.clawcrew.instances.<name>.package` to use another package, for example
`inputs.clawcrew.packages.${pkgs.stdenv.hostPlatform.system}.clawcrew` when your
system flake has a `clawcrew` input pointing at this repository.

## Single instance

Membership in `services.clawcrew.instances.<name>` is the activation signal;
there is no top-level `enable`. Each instance gets its own systemd unit, state
directory, and system user.

<div class="os-tabs-src">

#### nix

```nix
{ config, pkgs, ... }: {
  imports = [ ./path/to/clawcrew/nix/module.nix ];

  age.secrets.clawcrew-bot-token.file = ./secrets/clawcrew-bot-token.age;

  services.clawcrew.instances.me = {
    environmentFile = config.age.secrets.clawcrew-bot-token.path;
    settings = {
      providers.models.anthropic.home.model = "claude-sonnet-4-6";
      agents.assistant = {
        model_provider = "anthropic.home";
        risk_profile = "assistant";
        channels = [ "telegram.home" ];
      };
      risk_profiles.assistant = { };
      channels.telegram.home = {
        enabled = true;
        bot_token = "$BOT_TOKEN";   # systemd $VAR, substituted from environmentFile at start
        allowed_users = [ "12345" ];
      };
    };
  };
}
```

</div>

`settings` mirrors `~/.clawcrew/config.toml` as a Nix attrset, rendered to
`${dataDir}/config.toml` (mode `0600`). Secrets travel through
`environmentFile`, never `settings`: the unit's `ExecStartPre` runs `envsubst`
so `$VAR` references resolve at start, keeping the `/nix/store` copy free of
plaintext. The [config schema](../providers/configuration.md) (section headers,
type/alias convention) is identical to every other platform.

## Multiple instances

The module is `attrsOf submodule`-shaped, so N tenants on one host read the same
as one. Instances may share a user when exactly one creates it and the rest set
`createUser = false`.

<div class="os-tabs-src">

#### nix

```nix
services.clawcrew.instances = {
  alice = { environmentFile = "/run/secrets/alice/identity.env"; settings = { /* … */ }; };
  bob   = { environmentFile = "/run/secrets/bob/identity.env";   settings = { /* … */ }; };
};
```

</div>

## Options

The full option surface (`package`, `user`, `group`, `createUser`, `dataDir`,
`settings`, `environmentFile`, `extraConfig`, `bindReadOnlyPaths`) and the
secrets pattern are documented in
[`nix/README.md`](https://github.com/clawcrew-labs/clawcrew/blob/master/nix/README.md).
To override a `serviceConfig` field, use the standard NixOS escape hatch rather
than a module option:

<div class="os-tabs-src">

#### nix

```nix
systemd.services."clawcrew-me".serviceConfig.MemoryMax = "512M";
```

</div>

## Next

- [Service management](./service.md): the systemd unit ClawCrew generates on non-Nix hosts
- [Providers → Configuration](../providers/configuration.md): the config schema `settings` mirrors
