# The daemon as a NixOS service (docs/architecture.md, decisions 6 and 8):
# one store, one Unix socket, the socket's group the one door. nginx,
# oauth2-proxy and Kanidm in front are the host's to configure.
self:
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.embryo-systems;
  inherit (lib) mkOption types;
in
{
  options.services.embryo-systems = {
    enable = lib.mkEnableOption "the embryo-systems daemon, HBDS on a store";

    package = mkOption {
      type = types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
      defaultText = lib.literalExpression "embryo-systems.packages.\${system}.default";
      description = "The embryo-systems package.";
    };

    store = mkOption {
      type = types.path;
      default = "/var/lib/embryo-systems";
      description = "The store's directory: its journal and lock.";
    };

    socket = mkOption {
      type = types.path;
      default = "/run/embryo-systems/daemon.sock";
      description = "The Unix socket clients connect to.";
    };

    group = mkOption {
      type = types.str;
      default = "embryo-systems";
      description = "Who may connect: the socket is this group's.";
    };

    pace = mkOption {
      type = types.enum [
        "paused"
        "fast"
        "real"
      ];
      default = "paused";
      description = "How the store's time moves at start; POST /pace changes it live.";
    };

    factor = mkOption {
      type = types.nullOr types.float;
      default = null;
      description = "Real time's factor, with pace real.";
    };

    horizon = mkOption {
      type = types.nullOr types.float;
      default = null;
      description = "How far fast goes, with pace fast.";
    };

    slice = mkOption {
      type = types.ints.positive;
      default = 10000;
      description = "Instructions a client's turn runs before the next client's.";
    };

    compactAt = mkOption {
      type = types.ints.positive;
      default = 10000;
      description = "The journal's lines past which it is compacted.";
    };

    extraArgs = mkOption {
      type = types.listOf types.str;
      default = [ ];
      description = "More arguments to `embryo-systems daemon`.";
    };
  };

  config = lib.mkIf cfg.enable {
    users.users.embryo-systems = {
      isSystemUser = true;
      group = cfg.group;
    };
    users.groups.${cfg.group} = { };

    systemd.services.embryo-systems = {
      description = "embryo-systems: HBDS on a store";
      wantedBy = [ "multi-user.target" ];
      after = [ "local-fs.target" ];
      serviceConfig = {
        ExecStart = lib.escapeShellArgs (
          [
            (lib.getExe cfg.package)
            "daemon"
            "--socket"
            cfg.socket
            "--store"
            cfg.store
            "--pace"
            cfg.pace
            "--slice"
            (toString cfg.slice)
            "--compact-at"
            (toString cfg.compactAt)
          ]
          ++ lib.optionals (cfg.factor != null) [
            "--factor"
            (toString cfg.factor)
          ]
          ++ lib.optionals (cfg.horizon != null) [
            "--horizon"
            (toString cfg.horizon)
          ]
          ++ cfg.extraArgs
        );
        User = "embryo-systems";
        Group = cfg.group;
        StateDirectory = "embryo-systems";
        RuntimeDirectory = "embryo-systems";
        RuntimeDirectoryMode = "0750";
        # The socket is the group's: read and write for it, nothing for
        # others (decision 8's one door, guarded by file permissions).
        UMask = "0007";
        Restart = "on-failure";
        ReadWritePaths = [ cfg.store ];
        NoNewPrivileges = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        PrivateTmp = true;
        PrivateDevices = true;
        ProtectKernelTunables = true;
        ProtectKernelModules = true;
        ProtectControlGroups = true;
        RestrictAddressFamilies = [ "AF_UNIX" ];
        RestrictNamespaces = true;
        LockPersonality = true;
        MemoryDenyWriteExecute = true;
        SystemCallArchitectures = "native";
      };
    };
  };
}
