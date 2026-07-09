# rbx

rekordbox の master.db を操作する CLI ツール。
主に coding agent（Claude Code 等）から呼び出すことを想定した、エージェントファーストな設計。

## ⚠️ 先にライブラリをバックアップすること

rbx は rekordbox の master.db に直接書き込む。
`--execute` を伴うコマンドを実行する前に、**必ず rekordbox のライブラリをバックアップすること**。
rekordbox の 環境設定 → 詳細 → データベース → ライブラリーのバックアップ から取れる。

このソフトウェアは**いかなる保証もなし**で提供される（[ライセンス](#ライセンス)参照）。
バックアップせずに実行してライブラリが壊れても自己責任。

## インストール

[Releases ページ](../../releases) からプラットフォーム向けのビルドをダウンロードする:

| プラットフォーム | アセット |
|---|---|
| Linux / WSL (x86-64) | `rbx-<version>-x86_64-unknown-linux-gnu.tar.gz` |
| macOS (Intel) | `rbx-<version>-x86_64-apple-darwin.tar.gz` |
| macOS (Apple Silicon) | `rbx-<version>-aarch64-apple-darwin.tar.gz` |

```sh
tar xzf rbx-<version>-<target>.tar.gz
# 中のバイナリを PATH の通った場所に置く
```

同じリリースの `SHA256SUMS.txt` でダウンロードを検証すること。
Windows ネイティブ版はない。WSL 上で動かすこと。

自分でビルドする場合は[ビルド](#ビルド)を参照。

## 設計原則

- **Noun-Verb サブコマンド体系**: `rbx tracks list`, `rbx tracks mytags add` 等
- **全出力が構造化 JSON**: `schema_version` / `kind` / `items|item|error` の envelope
- **`describe` による自己記述**: リソース → アクション → フラグ・出力スキーマを階層的に発見可能
- **セマンティック終了コード**: 0=成功, 1=一般エラー, 2=使い方エラー, 3=未発見, 4=設定エラー, 5=競合
- **アクション可能なエラー**: `category` + `message` + `next_step` でリカバリを誘導
- **書き込みはデフォルト dry-run**: `--execute` 明示で実行。dry-run 時は `next_step` で案内

参考: [AIエージェントに自作CLIを効果的に使わせるための8原則](https://zenn.dev/assign/articles/b3d1d07d385b87)

## ビルド

```sh
cargo build --release
```

SQLCipher を static link するため、初回ビルドに時間がかかる。

## コマンドリファレンス

```sh
# DB パスは --db または環境変数で指定
export RBX_DB_PATH=/path/to/master.db
```

### スキーマ発見（DB不要）

```sh
rbx describe                           # リソース一覧
rbx describe tracks                    # tracks のアクション一覧
rbx describe tracks filter             # フラグ・出力スキーマ・例
```

### tracks（トラック操作）

```sh
rbx tracks list                        # 全曲一覧（ストリーミング専用を除く）
rbx tracks get <id>                    # ID指定で1曲取得
rbx tracks search 'query'             # タイトル・アーティスト検索
rbx tracks filter --bpm-min 125 --bpm-max 135 --key 8A --tag TAG_ID
                                       # BPM帯・キー・タグで絞り込み
rbx tracks update <id> --title '...' --artist '...' --bpm 128.0 \
    --key 8A --rating 4 --comment '...'
                                       # トラック情報更新（dry-run）
rbx tracks update <id> --bpm 130.0 --execute
                                       # トラック情報更新（実行）
```

### tracks cues（キューポイント操作）

```sh
rbx tracks cues list <track_id>        # MEMORY/HOT CUE 一覧
rbx tracks cues add <track_id> 12345   # MEMORY CUE 追加（dry-run）
rbx tracks cues add <track_id> 92000 --kind hot --slot 1 --comment 'Drop'
                                       # HOT CUE をスロット1に追加（dry-run）
rbx tracks cues update <cue_id> --msec 15000 --comment 'Verse'
                                       # CUE の位置・コメント変更（dry-run）
rbx tracks cues delete <cue_id>        # CUE 削除（dry-run）
```

### tracks mytags（トラックへのタグ付け外し）

```sh
rbx tracks mytags list <track_id>      # トラックに付いてるタグ一覧
rbx tracks mytags add <track_id> <tag_id>
                                       # タグ付け（dry-run）
rbx tracks mytags remove <track_id> <tag_id>
                                       # タグ外し（dry-run）
```

### playlists（プレイリスト操作）

```sh
rbx playlists list                     # プレイリスト・フォルダ一覧
rbx playlists tracks list <playlist_id>
                                       # プレイリスト内のトラック一覧
rbx playlists tracks add <playlist_id> <track_id>
                                       # プレイリストに曲追加（dry-run）
rbx playlists tracks remove <playlist_id> <track_id>
                                       # プレイリストから曲除去（dry-run）
rbx playlists search <track_id>        # 曲が所属するプレイリスト逆引き
rbx playlists create '名前' --parent FOLDER_ID
                                       # プレイリスト新規作成（dry-run）
rbx playlists delete <id>              # プレイリスト削除（dry-run）
```

### mytags（マイタグ自体の管理）

```sh
rbx mytags list                        # カテゴリ・タグ一覧
rbx mytags tracks <tag_id>             # タグに紐づくトラック一覧
rbx mytags create '名前'              # トップレベルカテゴリ作成（dry-run）
rbx mytags create '名前' --parent CATEGORY_ID
                                       # カテゴリ配下にタグ作成（dry-run）
rbx mytags delete <id>                 # タグ削除（dry-run）
```

### history（再生履歴）

```sh
rbx history list                       # 直近の再生セッション（デフォルト20件）
rbx history list --limit 5             # 件数指定
rbx history tracks <session_id>        # セッション内のトラック（再生順）
```

### 生SQL

```sh
rbx query 'SELECT ID, Title FROM djmdContent LIMIT 5'
rbx query 'PRAGMA table_info(djmdContent)'
```

`query` は read-only（SELECT / WITH / PRAGMA / EXPLAIN の単文のみ）。
書き込みには `--unsafe-write` が必要だが、
専用コマンドが維持している rekordbox の不変条件
（USN採番・タイムスタンプ形式・数値ID・masterPlaylists6.xml 同期）をすべてバイパスする。
原則専用コマンドを使うこと。

上記で `(dry-run)` と書いてあるコマンドは、デフォルトでは変更内容のプレビューだけを返す。
`--execute` を付けると実際に書き込む。

## エージェントから使う場合

`rbx describe` から始めれば、利用可能なリソース・アクション・フラグ・出力スキーマを階層的に発見できる。
出力は常に JSON なので `jq` でそのまま処理可能。

## rekordbox master.db について

rekordbox 6 以降の master.db は SQLCipher で暗号化されている。
rbx は固定パスワードで自動的に復号するため、ユーザーが意識する必要はない。

## ライセンス

MIT. [LICENSE](LICENSE) を参照。

本ソフトウェアは "現状のまま" 提供され、いかなる保証もない。
作者は rekordbox ライブラリやデータへのいかなる損害についても責任を負わない。
バックアップを取ること。
