# Relay Server

<!-- Problem Statement -->
Triggering a repository workflow
in response to a commit on a different repository
is not a trivial problem.
This is particularly useful 
for projects that have git dependencies.
Triggering a CI workflow
when a git dependency gets updated
is important for detecting breaking changes as soon as possible.

<!-- Solution Presentation -->
This project attempts to solve this problem
by providing a server that acts as an intermediary
between the repositories containing the dependencies
and the repositories that need those dependencies.
This project uses
[`axum`] to handle incoming requests,
[`reqwest`] to send requests to the GitHub API,
[`git ls-remote`] to check the last commit on a remote branch,
and [`sqlx`] connected to a SQLite database to hold state.

<!-- Links -->
[`axum`]: https://docs.rs/axum/latest/axum/
[`reqwest`]: https://docs.rs/reqwest/latest/reqwest/
[`git ls-remote`]: https://git-scm.com/docs/git-ls-remote
[`sqlx`]: https://docs.rs/sqlx/latest/sqlx/

## Usage

<!-- GitHub App Setup -->
To authorize the server to trigger a workflow in your repository,
set up and install a GitHub App
with `Contents` set to `Read and write` permissions.


<!-- Workflow Setup -->
Set up your GitHub Actions workflow
to be triggered by a `repository_dispatch` event:

```yaml
on:
  repository_dispatch:
    types: [EVENT_TYPE]
```

`EVENT_TYPE` is a string containing up to 100 characters.
It is used to distinguish the event
from other `repository_dispatch` events.

<!-- Running Server -->
Clone this repository:

```shell
git clone https://github.com/Nilirad/relay.git
```

Configure the environment according to the section below,
then run the server:

```shell
cargo run --release
```

<!-- Populate Database -->
Populate the database with the subscriptions you need:

```shell
curl -X POST http://localhost:3000/subscribers \
  -H "Content-Type: application/json" \
  -d '{
    "source_repo_url": "SOURCE_REPOSITORY",
    "source_branch_name": "BRANCH_NAME",
    "target_repo": "YOUR_REPOSITORY",
    "event_type": "EVENT_TYPE",
    "gh_app_installation_id": YOUR_INSTALLATION_ID
  }'
```

Make sure that `EVENT_TYPE` is the same
as the one defined in the workflow.

<!-- Wait for changes -->
At this point,
the server is ready to listen to the source repository
and trigger your workflow shortly after a new commit is pushed
(about 5 minutes or less).

## Environment Configuration

[`Nix`] is recommended to set up the development environment.
If you decide to use it,
just run `nix develop`
to enter a shell with the required environment.
If you use [`nix-direnv`],
you can automatically enter the shell just by entering the workspace directory.
In any case, ensure [flakes are enabled]
and that you have set the `GH_CLIENT_ID` and the `GH_APP_KEY_PATH`
environment variables (see below).

If you prefer not to use Nix,
you can still build and run this server by manually configuring the environment:

- Install [Rust]
  (build-time dependency).
- Install `git`
  (runtime dependency).
- Set up the `GH_CLIENT_ID` environment variable
  with the Client ID of the GitHub App.
- Set up the `GH_APP_KEY_PATH` environment variable
  with the path to the `.pem` key.

<!-- Links -->
[`Nix`]: https://nixos.org/learn/
[`nix-direnv`]: https://github.com/nix-community/nix-direnv
[flakes are enabled]: https://nixos.wiki/wiki/Flakes
[Rust]: https://rust-lang.org/learn/get-started/

## License

This repository is dual-licensed under the following,
unless otherwise noted:

- [MIT LICENSE][mit]
- [Apache License, Version 2.0][apache]

at your option.

<!-- LINKS -->

[mit]: https://opensource.org/license/mit
[apache]: https://www.apache.org/licenses/LICENSE-2.0
