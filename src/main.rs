use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Debug, Default)]
struct Config {
    username: String,
    title: String,
    base_path: String,
    site_root: String,
    bootstrap_months: i64,
    allowlist: BTreeSet<String>,
    keywords: Vec<String>,
    exclude: BTreeSet<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Event {
    id: String,
    event_type: String,
    repo: String,
    title: String,
    url: String,
    occurred_at: String,
    thread_id: String,
    thread_title: String,
    thread_url: String,
    status: String,
}

#[derive(Clone, Debug, Default)]
struct Feed {
    generated_at: String,
    username: String,
    events: Vec<Event>,
}

#[derive(Clone, Debug)]
enum Json {
    Null,
    Bool(()),
    Number(f64),
    String(String),
    Array(Vec<Json>),
    Object(BTreeMap<String, Json>),
}

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let mut args = env::args().skip(1);
    let Some(cmd) = args.next() else {
        return usage();
    };
    let rest: Vec<String> = args.collect();
    match cmd.as_str() {
        "collect" => collect_cmd(&rest),
        "render" => render_cmd(&rest),
        "validate" => validate_cmd(&rest),
        "fixture" => fixture_cmd(&rest),
        _ => usage(),
    }
}

fn usage() -> Result<()> {
    Err("usage: btc-contribs <collect|render|validate|fixture> [--config PATH] [--feed PATH] [--state PATH] [--out PATH]".into())
}

fn collect_cmd(args: &[String]) -> Result<()> {
    let config_path = flag(args, "--config").unwrap_or_else(|| "config/site.toml".into());
    let state_path = flag(args, "--state").unwrap_or_else(|| ".cache/feed.json".into());
    let out_path = flag(args, "--out").unwrap_or_else(|| "public/feed.json".into());
    let candidates_path =
        flag(args, "--candidates").unwrap_or_else(|| ".cache/candidates.md".into());
    let full = args.iter().any(|a| a == "--full")
        || env::var("BTC_CONTRIBS_FULL").is_ok_and(|v| v == "1" || v == "true");
    let config = Config::from_file(Path::new(&config_path))?;
    let token = env::var("GITHUB_TOKEN")
        .or_else(|_| env::var("GH_TOKEN"))
        .map_err(|_| "set GITHUB_TOKEN or GH_TOKEN for collection".to_string())?;

    let mut feed = Feed {
        generated_at: now_rfc3339(),
        username: config.username.clone(),
        events: Vec::new(),
    };
    if Path::new(&state_path).exists() && !full {
        feed = Feed::from_json_file(Path::new(&state_path))?;
        feed.generated_at = now_rfc3339();
    }

    let from = if full {
        "1970-01-01T00:00:00Z".to_string()
    } else {
        months_ago_rfc3339(config.bootstrap_months)
    };
    let to = now_rfc3339();
    let graph = fetch_contributions(&token, &config.username, &from, &to)?;
    let (events, candidates) = extract_contributions(&config, &graph)?;
    merge_events(&mut feed.events, events);
    let comments = fetch_comment_events(&token, &config, &from)?;
    merge_events(&mut feed.events, comments);
    feed.events
        .sort_by(|a, b| b.occurred_at.cmp(&a.occurred_at).then(a.id.cmp(&b.id)));

    write_parented(Path::new(&out_path), &feed.to_json())?;
    write_parented(Path::new(&state_path), &feed.to_json())?;

    if !candidates.is_empty() {
        let report = candidate_report(&config, &candidates);
        write_parented(Path::new(&candidates_path), &report)?;
        if env::var("GITHUB_REPOSITORY").is_ok() {
            update_candidate_issue(&token, &report)?;
        }
    }
    Ok(())
}

fn render_cmd(args: &[String]) -> Result<()> {
    let config_path = flag(args, "--config").unwrap_or_else(|| "config/site.toml".into());
    let feed_path = flag(args, "--feed").unwrap_or_else(|| "public/feed.json".into());
    let out_dir = flag(args, "--out").unwrap_or_else(|| "public".into());
    let config = Config::from_file(Path::new(&config_path))?;
    let feed = Feed::from_json_file(Path::new(&feed_path))?;
    let out = PathBuf::from(out_dir);
    fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    fs::write(out.join("index.html"), render_html(&config, &feed)).map_err(|e| e.to_string())?;
    fs::write(out.join("btc_foss.css"), render_css()).map_err(|e| e.to_string())?;
    fs::write(out.join("btc_foss.js"), render_js()).map_err(|e| e.to_string())?;
    if !out.join("feed.json").exists() {
        fs::write(out.join("feed.json"), feed.to_json()).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn validate_cmd(args: &[String]) -> Result<()> {
    let config_path = flag(args, "--config").unwrap_or_else(|| "config/site.toml".into());
    let config = Config::from_file(Path::new(&config_path))?;
    if config.username.is_empty() {
        return Err("config username must not be empty".into());
    }
    if !config.base_path.starts_with('/') || !config.base_path.ends_with('/') {
        return Err("base_path must start and end with '/'".into());
    }
    Ok(())
}

fn fixture_cmd(args: &[String]) -> Result<()> {
    let config_path = flag(args, "--config").unwrap_or_else(|| "config/site.toml".into());
    let feed_path = flag(args, "--feed").unwrap_or_else(|| "fixtures/feed.json".into());
    let out_dir = flag(args, "--out").unwrap_or_else(|| "public".into());
    render_cmd(&[
        "--config".into(),
        config_path,
        "--feed".into(),
        feed_path,
        "--out".into(),
        out_dir,
    ])
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.windows(2)
        .find_map(|w| (w[0] == name).then(|| w[1].clone()))
}

impl Config {
    fn from_file(path: &Path) -> Result<Self> {
        let raw = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut cfg = Config {
            title: "Bitcoin FOSS Contributions".into(),
            base_path: "/btc_foss/".into(),
            site_root: "https://trevs.site".into(),
            bootstrap_months: 5,
            ..Config::default()
        };
        for line in logical_toml_lines(&raw) {
            let line = line.split('#').next().unwrap_or("").trim().to_string();
            if line.is_empty() || line.starts_with('[') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            let value = value.trim();
            match key {
                "username" => cfg.username = parse_toml_string(value)?,
                "title" => cfg.title = parse_toml_string(value)?,
                "base_path" => cfg.base_path = parse_toml_string(value)?,
                "site_root" => cfg.site_root = parse_toml_string(value)?,
                "bootstrap_months" => {
                    cfg.bootstrap_months = value
                        .parse()
                        .map_err(|_| "invalid bootstrap_months".to_string())?
                }
                "allowlist" => cfg.allowlist = parse_toml_array(value)?.into_iter().collect(),
                "keywords" => cfg.keywords = parse_toml_array(value)?,
                "exclude" => cfg.exclude = parse_toml_array(value)?.into_iter().collect(),
                _ => {}
            }
        }
        if cfg.username.is_empty() {
            return Err("config/site.toml must set username".into());
        }
        Ok(cfg)
    }
}

fn logical_toml_lines(raw: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut in_array = false;
    for raw_line in raw.lines() {
        let line = raw_line.trim();
        if line.is_empty() && !in_array {
            continue;
        }
        if in_array {
            current.push(' ');
            current.push_str(line);
            if line.contains(']') {
                lines.push(current.trim().to_string());
                current.clear();
                in_array = false;
            }
            continue;
        }
        if line.contains('=') && line.contains('[') && !line.contains(']') {
            current.push_str(line);
            in_array = true;
        } else {
            lines.push(line.to_string());
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

fn parse_toml_string(value: &str) -> Result<String> {
    let value = value.trim();
    if value.starts_with('"') && value.ends_with('"') && value.len() >= 2 {
        Ok(unescape_json_string(&value[1..value.len() - 1]))
    } else {
        Err(format!("expected TOML string, got {value}"))
    }
}

fn parse_toml_array(value: &str) -> Result<Vec<String>> {
    let value = value.trim();
    if value == "[]" {
        return Ok(Vec::new());
    }
    if !value.starts_with('[') || !value.ends_with(']') {
        return Err(format!("expected TOML string array, got {value}"));
    }
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_str = false;
    let mut escape = false;
    for ch in value[1..value.len() - 1].chars() {
        if escape {
            cur.push(ch);
            escape = false;
        } else if ch == '\\' && in_str {
            escape = true;
        } else if ch == '"' {
            if in_str {
                out.push(cur.clone());
                cur.clear();
            }
            in_str = !in_str;
        } else if in_str {
            cur.push(ch);
        }
    }
    Ok(out)
}

fn fetch_contributions(token: &str, login: &str, from: &str, to: &str) -> Result<Json> {
    let query = r#"
query($login:String!, $from:DateTime!, $to:DateTime!) {
  user(login:$login) {
    contributionsCollection(from:$from, to:$to) {
      pullRequestContributionsByRepository(maxRepositories:100) {
        repository { nameWithOwner description repositoryTopics(first:20) { nodes { topic { name } } } }
        contributions(first:100) { nodes { pullRequest { id number title url createdAt state closed merged } } }
      }
      issueContributionsByRepository(maxRepositories:100) {
        repository { nameWithOwner description repositoryTopics(first:20) { nodes { topic { name } } } }
        contributions(first:100) { nodes { issue { id number title url createdAt state closed } } }
      }
      commitContributionsByRepository(maxRepositories:100) {
        repository { nameWithOwner description repositoryTopics(first:20) { nodes { topic { name } } } }
        contributions(first:100) { nodes { commitCount occurredAt url } }
      }
      pullRequestReviewContributionsByRepository(maxRepositories:100) {
        repository { nameWithOwner description repositoryTopics(first:20) { nodes { topic { name } } } }
        contributions(first:100) { nodes { pullRequestReview { id url createdAt state pullRequest { number title url } } } }
      }
    }
  }
}
"#;
    let body = format!(
        r#"{{"query":"{}","variables":{{"login":"{}","from":"{}","to":"{}"}}}}"#,
        json_escape(query),
        json_escape(login),
        json_escape(from),
        json_escape(to)
    );
    let output = Command::new("curl")
        .args([
            "-sS",
            "-X",
            "POST",
            "-H",
            &format!("Authorization: bearer {token}"),
            "-H",
            "Content-Type: application/json",
            "-d",
            &body,
            "https://api.github.com/graphql",
        ])
        .output()
        .map_err(|e| format!("failed to run curl: {e}"))?;
    if !output.status.success() {
        return Err(format!("curl failed with status {}", output.status));
    }
    let text = String::from_utf8(output.stdout).map_err(|e| e.to_string())?;
    let json = parse_json(&text)?;
    if json.get_path(&["errors"]).is_some() {
        return Err(format!("GitHub GraphQL returned errors: {text}"));
    }
    Ok(json)
}

fn extract_contributions(config: &Config, graph: &Json) -> Result<(Vec<Event>, BTreeSet<String>)> {
    let coll = graph
        .get_path(&["data", "user", "contributionsCollection"])
        .ok_or_else(|| "missing contributionsCollection in GraphQL response".to_string())?;
    let mut events = Vec::new();
    let mut candidates = BTreeSet::new();
    let groups = [
        ("pullRequestContributionsByRepository", "pull_request"),
        ("issueContributionsByRepository", "issue"),
        ("commitContributionsByRepository", "commit"),
        ("pullRequestReviewContributionsByRepository", "review"),
    ];
    for (field, kind) in groups {
        for group in coll.get(field).and_then(Json::array).unwrap_or(&[]) {
            let repo = group
                .get_path(&["repository", "nameWithOwner"])
                .and_then(Json::string)
                .unwrap_or("");
            if repo.is_empty() || config.exclude.contains(repo) {
                continue;
            }
            if !config.allowlist.contains(repo)
                && repo_matches_keywords(group.get("repository"), &config.keywords)
            {
                candidates.insert(repo.to_string());
            }
            if !config.allowlist.contains(repo) {
                continue;
            }
            for node in group
                .get_path(&["contributions", "nodes"])
                .and_then(Json::array)
                .unwrap_or(&[])
            {
                if let Some(event) = event_from_node(kind, repo, node) {
                    events.push(event);
                }
            }
        }
    }
    Ok((events, candidates))
}

fn event_from_node(kind: &str, repo: &str, node: &Json) -> Option<Event> {
    match kind {
        "pull_request" => {
            let pr = node.get("pullRequest")?;
            let id = pr.get("id")?.string()?.to_string();
            let number = pr.get("number").and_then(Json::number).unwrap_or(0.0) as i64;
            let title = pr.get("title")?.string()?.to_string();
            let url = pr.get("url")?.string()?.to_string();
            Some(Event {
                id,
                event_type: kind.into(),
                repo: repo.into(),
                title: title.clone(),
                url: url.clone(),
                occurred_at: pr.get("createdAt")?.string()?.to_string(),
                thread_id: format!("{repo}#{number}"),
                thread_title: title,
                thread_url: url,
                status: pr.get("state").and_then(Json::string).unwrap_or("").into(),
            })
        }
        "issue" => {
            let issue = node.get("issue")?;
            let id = issue.get("id")?.string()?.to_string();
            let number = issue.get("number").and_then(Json::number).unwrap_or(0.0) as i64;
            let title = issue.get("title")?.string()?.to_string();
            let url = issue.get("url")?.string()?.to_string();
            Some(Event {
                id,
                event_type: kind.into(),
                repo: repo.into(),
                title: title.clone(),
                url: url.clone(),
                occurred_at: issue.get("createdAt")?.string()?.to_string(),
                thread_id: format!("{repo}#{number}"),
                thread_title: title,
                thread_url: url,
                status: issue
                    .get("state")
                    .and_then(Json::string)
                    .unwrap_or("")
                    .into(),
            })
        }
        "commit" => {
            let date = node.get("occurredAt")?.string()?.to_string();
            let count = node
                .get("commitCount")
                .and_then(Json::number)
                .unwrap_or(1.0) as i64;
            let url = node
                .get("url")
                .and_then(Json::string)
                .unwrap_or("")
                .to_string();
            Some(Event {
                id: format!("{repo}:commit:{date}:{count}"),
                event_type: kind.into(),
                repo: repo.into(),
                title: format!("{count} commit{}", if count == 1 { "" } else { "s" }),
                url: url.clone(),
                occurred_at: date.clone(),
                thread_id: format!("{repo}:commits:{date}"),
                thread_title: "Commits".into(),
                thread_url: url,
                status: String::new(),
            })
        }
        "review" => {
            let review = node.get("pullRequestReview")?;
            let pr = review.get("pullRequest")?;
            let number = pr.get("number").and_then(Json::number).unwrap_or(0.0) as i64;
            let thread_title = pr.get("title")?.string()?.to_string();
            let thread_url = pr.get("url")?.string()?.to_string();
            let status = review
                .get("state")
                .and_then(Json::string)
                .unwrap_or("")
                .to_string();
            Some(Event {
                id: review.get("id")?.string()?.to_string(),
                event_type: kind.into(),
                repo: repo.into(),
                title: format!("Reviewed {thread_title}"),
                url: review.get("url")?.string()?.to_string(),
                occurred_at: review.get("createdAt")?.string()?.to_string(),
                thread_id: format!("{repo}#{number}"),
                thread_title,
                thread_url,
                status,
            })
        }
        _ => None,
    }
}

fn fetch_comment_events(token: &str, config: &Config, from: &str) -> Result<Vec<Event>> {
    let mut events = Vec::new();
    let since = from.get(0..10).unwrap_or(from);
    for repo in &config.allowlist {
        let query_text = format!(
            "repo:{repo} commenter:{} updated:>={since}",
            config.username
        );
        let query = r#"
query($query:String!) {
  search(query:$query, type:ISSUE, first:100) {
    nodes {
      ... on Issue {
        number title url
        repository { nameWithOwner }
        comments(first:100) { nodes { id url createdAt author { login } } }
      }
      ... on PullRequest {
        number title url
        repository { nameWithOwner }
        comments(first:100) { nodes { id url createdAt author { login } } }
      }
    }
  }
}
"#;
        let body = format!(
            r#"{{"query":"{}","variables":{{"query":"{}"}}}}"#,
            json_escape(query),
            json_escape(&query_text)
        );
        let output = Command::new("curl")
            .args([
                "-sS",
                "-X",
                "POST",
                "-H",
                &format!("Authorization: bearer {token}"),
                "-H",
                "Content-Type: application/json",
                "-d",
                &body,
                "https://api.github.com/graphql",
            ])
            .output()
            .map_err(|e| format!("failed to run curl: {e}"))?;
        if !output.status.success() {
            return Err(format!("curl failed with status {}", output.status));
        }
        let text = String::from_utf8(output.stdout).map_err(|e| e.to_string())?;
        let json = parse_json(&text)?;
        if json.get_path(&["errors"]).is_some() {
            return Err(format!("GitHub comment search returned errors: {text}"));
        }
        for thread in json
            .get_path(&["data", "search", "nodes"])
            .and_then(Json::array)
            .unwrap_or(&[])
        {
            let repo = thread
                .get_path(&["repository", "nameWithOwner"])
                .and_then(Json::string)
                .unwrap_or("");
            let number = thread.get("number").and_then(Json::number).unwrap_or(0.0) as i64;
            let thread_title = thread
                .get("title")
                .and_then(Json::string)
                .unwrap_or("")
                .to_string();
            let thread_url = thread
                .get("url")
                .and_then(Json::string)
                .unwrap_or("")
                .to_string();
            for comment in thread
                .get_path(&["comments", "nodes"])
                .and_then(Json::array)
                .unwrap_or(&[])
            {
                if comment
                    .get_path(&["author", "login"])
                    .and_then(Json::string)
                    != Some(config.username.as_str())
                {
                    continue;
                }
                events.push(Event {
                    id: comment
                        .get("id")
                        .and_then(Json::string)
                        .unwrap_or("")
                        .to_string(),
                    event_type: "comment".into(),
                    repo: repo.into(),
                    title: format!("Commented on {thread_title}"),
                    url: comment
                        .get("url")
                        .and_then(Json::string)
                        .unwrap_or("")
                        .to_string(),
                    occurred_at: comment
                        .get("createdAt")
                        .and_then(Json::string)
                        .unwrap_or("")
                        .to_string(),
                    thread_id: format!("{repo}#{number}"),
                    thread_title: thread_title.clone(),
                    thread_url: thread_url.clone(),
                    status: String::new(),
                });
            }
        }
    }
    Ok(events)
}

fn repo_matches_keywords(repo: Option<&Json>, keywords: &[String]) -> bool {
    let Some(repo) = repo else {
        return false;
    };
    let mut haystack = String::new();
    if let Some(name) = repo.get("nameWithOwner").and_then(Json::string) {
        haystack.push_str(name);
        haystack.push(' ');
    }
    if let Some(desc) = repo.get("description").and_then(Json::string) {
        haystack.push_str(desc);
        haystack.push(' ');
    }
    if let Some(topics) = repo
        .get_path(&["repositoryTopics", "nodes"])
        .and_then(Json::array)
    {
        for topic in topics {
            if let Some(name) = topic.get_path(&["topic", "name"]).and_then(Json::string) {
                haystack.push_str(name);
                haystack.push(' ');
            }
        }
    }
    let haystack = haystack.to_lowercase();
    keywords
        .iter()
        .any(|k| haystack.contains(&k.to_lowercase()))
}

fn merge_events(existing: &mut Vec<Event>, incoming: Vec<Event>) {
    let mut by_id: BTreeMap<String, Event> =
        existing.drain(..).map(|e| (e.id.clone(), e)).collect();
    for event in incoming {
        by_id.insert(event.id.clone(), event);
    }
    *existing = by_id.into_values().collect();
}

fn candidate_report(config: &Config, candidates: &BTreeSet<String>) -> String {
    let mut out = String::new();
    writeln!(out, "# Bitcoin FOSS candidate repositories\n").unwrap();
    writeln!(
        out,
        "Discovered from public GitHub activity for `{}` using Bitcoin-only keywords.",
        config.username
    )
    .unwrap();
    writeln!(
        out,
        "Curate candidates by adding approved repositories to `config/site.toml` `allowlist`.\n"
    )
    .unwrap();
    for repo in candidates {
        writeln!(out, "- [ ] `{repo}` - https://github.com/{repo}").unwrap();
    }
    out
}

fn update_candidate_issue(token: &str, body: &str) -> Result<()> {
    let repo = env::var("GITHUB_REPOSITORY").map_err(|e| e.to_string())?;
    let title = "Bitcoin FOSS repository candidates";
    let list_url =
        format!("https://api.github.com/repos/{repo}/issues?state=open&labels=btc-foss-candidates");
    let list = curl_json(token, "GET", &list_url, None)?;
    let issue_number = list.array().and_then(|issues| {
        issues.iter().find_map(|issue| {
            (issue.get("title").and_then(Json::string) == Some(title))
                .then(|| issue.get("number").and_then(Json::number).unwrap_or(0.0) as i64)
        })
    });
    let payload = format!(
        r#"{{"title":"{}","body":"{}","labels":["btc-foss-candidates"]}}"#,
        json_escape(title),
        json_escape(body)
    );
    if let Some(number) = issue_number {
        let url = format!("https://api.github.com/repos/{repo}/issues/{number}");
        curl_json(token, "PATCH", &url, Some(&payload))?;
    } else {
        let url = format!("https://api.github.com/repos/{repo}/issues");
        curl_json(token, "POST", &url, Some(&payload))?;
    }
    Ok(())
}

fn curl_json(token: &str, method: &str, url: &str, body: Option<&str>) -> Result<Json> {
    let mut cmd = Command::new("curl");
    cmd.args([
        "-sS",
        "-X",
        method,
        "-H",
        &format!("Authorization: bearer {token}"),
        "-H",
        "Accept: application/vnd.github+json",
        "-H",
        "X-GitHub-Api-Version: 2022-11-28",
    ]);
    if let Some(body) = body {
        cmd.args(["-H", "Content-Type: application/json", "-d", body]);
    }
    let output = cmd.arg(url).output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(format!("curl {method} {url} failed: {}", output.status));
    }
    let text = String::from_utf8(output.stdout).map_err(|e| e.to_string())?;
    parse_json(&text)
}

impl Feed {
    fn from_json_file(path: &Path) -> Result<Self> {
        let raw = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let json = parse_json(&raw)?;
        let generated_at = json
            .get("generated_at")
            .and_then(Json::string)
            .unwrap_or("")
            .to_string();
        let username = json
            .get("username")
            .and_then(Json::string)
            .unwrap_or("")
            .to_string();
        let mut events = Vec::new();
        for item in json.get("events").and_then(Json::array).unwrap_or(&[]) {
            events.push(Event {
                id: item
                    .get("id")
                    .and_then(Json::string)
                    .unwrap_or("")
                    .to_string(),
                event_type: item
                    .get("event_type")
                    .and_then(Json::string)
                    .unwrap_or("")
                    .to_string(),
                repo: item
                    .get("repo")
                    .and_then(Json::string)
                    .unwrap_or("")
                    .to_string(),
                title: item
                    .get("title")
                    .and_then(Json::string)
                    .unwrap_or("")
                    .to_string(),
                url: item
                    .get("url")
                    .and_then(Json::string)
                    .unwrap_or("")
                    .to_string(),
                occurred_at: item
                    .get("occurred_at")
                    .and_then(Json::string)
                    .unwrap_or("")
                    .to_string(),
                thread_id: item
                    .get("thread_id")
                    .and_then(Json::string)
                    .unwrap_or("")
                    .to_string(),
                thread_title: item
                    .get("thread_title")
                    .and_then(Json::string)
                    .unwrap_or("")
                    .to_string(),
                thread_url: item
                    .get("thread_url")
                    .and_then(Json::string)
                    .unwrap_or("")
                    .to_string(),
                status: item
                    .get("status")
                    .and_then(Json::string)
                    .unwrap_or("")
                    .to_string(),
            });
        }
        Ok(Feed {
            generated_at,
            username,
            events,
        })
    }

    fn to_json(&self) -> String {
        let mut out = String::new();
        writeln!(out, "{{").unwrap();
        writeln!(
            out,
            "  \"generated_at\": \"{}\",",
            json_escape(&self.generated_at)
        )
        .unwrap();
        writeln!(out, "  \"username\": \"{}\",", json_escape(&self.username)).unwrap();
        writeln!(out, "  \"events\": [").unwrap();
        for (i, event) in self.events.iter().enumerate() {
            if i > 0 {
                writeln!(out, ",").unwrap();
            }
            write!(
                out,
                "    {{\n      \"id\": \"{}\",\n      \"event_type\": \"{}\",\n      \"repo\": \"{}\",\n      \"title\": \"{}\",\n      \"url\": \"{}\",\n      \"occurred_at\": \"{}\",\n      \"thread_id\": \"{}\",\n      \"thread_title\": \"{}\",\n      \"thread_url\": \"{}\",\n      \"status\": \"{}\"\n    }}",
                json_escape(&event.id),
                json_escape(&event.event_type),
                json_escape(&event.repo),
                json_escape(&event.title),
                json_escape(&event.url),
                json_escape(&event.occurred_at),
                json_escape(&event.thread_id),
                json_escape(&event.thread_title),
                json_escape(&event.thread_url),
                json_escape(&event.status)
            ).unwrap();
        }
        writeln!(out, "\n  ]\n}}").unwrap();
        out
    }
}

fn render_html(config: &Config, feed: &Feed) -> String {
    let repos: BTreeSet<_> = feed.events.iter().map(|e| e.repo.as_str()).collect();
    let types: BTreeSet<_> = feed.events.iter().map(|e| e.event_type.as_str()).collect();
    let years: BTreeSet<_> = feed
        .events
        .iter()
        .filter_map(|e| e.occurred_at.get(0..4))
        .collect();
    let mut type_counts: BTreeMap<&str, usize> = BTreeMap::new();
    for event in &feed.events {
        *type_counts.entry(&event.event_type).or_default() += 1;
    }

    let mut out = String::new();
    writeln!(out, "<!DOCTYPE html>").unwrap();
    writeln!(out, "<html lang=\"en\"><head><meta charset=\"UTF-8\">").unwrap();
    writeln!(
        out,
        "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">"
    )
    .unwrap();
    writeln!(
        out,
        "<link rel=\"shortcut icon\" type=\"image/png\" href=\"/favicon.png\">"
    )
    .unwrap();
    writeln!(out, "<link rel=\"stylesheet\" href=\"/static/style.css\">").unwrap();
    writeln!(
        out,
        "<link rel=\"stylesheet\" href=\"{}btc_foss.css\">",
        html_attr(&config.base_path)
    )
    .unwrap();
    writeln!(out, "<title>{}</title></head><body>", html(&config.title)).unwrap();
    writeln!(out, "<header><div class=\"markdown-heading\"><h1 class=\"heading-element\">trev's website</h1></div><p><a href=\"/index.html\">Home</a><a href=\"/posts/index.html\">Posts</a><a href=\"/about/index.html\">About</a></p></header>").unwrap();
    writeln!(out, "<main class=\"btc-page\">").unwrap();
    writeln!(out, "<h1>{}</h1>", html(&config.title)).unwrap();
    writeln!(out, "<p class=\"btc-muted\">Public GitHub activity for <a href=\"https://github.com/{0}\">{0}</a>. Updated <time datetime=\"{1}\">{1}</time>. <a href=\"{2}feed.json\">feed.json</a></p>", html_attr(&feed.username), html_attr(&feed.generated_at), html_attr(&config.base_path)).unwrap();
    writeln!(
        out,
        "<section class=\"btc-stats\" aria-label=\"Contribution summary\">"
    )
    .unwrap();
    stat(&mut out, "Events", feed.events.len());
    stat(&mut out, "Repos", repos.len());
    for (event_type, count) in &type_counts {
        stat(&mut out, event_type, *count);
    }
    writeln!(out, "</section>").unwrap();
    writeln!(out, "<form class=\"btc-filters\" id=\"btc-filters\">").unwrap();
    select(&mut out, "repo", "Repository", repos.iter().copied());
    select(&mut out, "type", "Type", types.iter().copied());
    select(&mut out, "year", "Year", years.iter().copied().rev());
    writeln!(
        out,
        "<a class=\"btc-reset\" href=\"{}\">Reset</a></form>",
        html_attr(&config.base_path)
    )
    .unwrap();
    writeln!(out, "<div class=\"btc-timeline\" id=\"btc-timeline\">").unwrap();

    let mut grouped: BTreeMap<&str, Vec<&Event>> = BTreeMap::new();
    for event in &feed.events {
        grouped.entry(&event.thread_id).or_default().push(event);
    }
    let mut groups: Vec<_> = grouped.into_values().collect();
    groups.sort_by(|a, b| b[0].occurred_at.cmp(&a[0].occurred_at));
    for group in groups {
        let first = group[0];
        let year = first.occurred_at.get(0..4).unwrap_or("");
        writeln!(
            out,
            "<details class=\"btc-thread\" open data-repo=\"{}\" data-type=\"{}\" data-year=\"{}\">",
            html_attr(&first.repo),
            html_attr(&first.event_type),
            html_attr(year)
        ).unwrap();
        writeln!(out, "<summary><span class=\"btc-icon\">{}</span><span><a href=\"{}\">{}</a><small>{} · {} item{}</small></span></summary>",
            icon(&first.event_type), html_attr(&first.thread_url), html(&first.thread_title), html(&first.repo), group.len(), if group.len() == 1 { "" } else { "s" }).unwrap();
        writeln!(out, "<ol>").unwrap();
        for event in group {
            writeln!(out, "<li data-type=\"{}\"><time datetime=\"{}\">{}</time> <span class=\"btc-kind\">{}</span> <a href=\"{}\">{}</a>{}</li>",
                html_attr(&event.event_type),
                html_attr(&event.occurred_at),
                html(&short_date(&event.occurred_at)),
                html(&event.event_type.replace('_', " ")),
                html_attr(&event.url),
                html(&event.title),
                status_badge(&event.status)).unwrap();
        }
        writeln!(out, "</ol></details>").unwrap();
    }
    writeln!(out, "</div></main><footer><p>Trevor Arjeski - <a href=\"https://github.com/trevarj\">git</a> <a href=\"/rss.xml\">rss</a></p></footer>").unwrap();
    writeln!(
        out,
        "<script defer src=\"{}btc_foss.js\"></script>",
        html_attr(&config.base_path)
    )
    .unwrap();
    writeln!(out, "</body></html>").unwrap();
    out
}

fn stat(out: &mut String, label: &str, value: usize) {
    writeln!(
        out,
        "<div><strong>{value}</strong><span>{}</span></div>",
        html(&label.replace('_', " "))
    )
    .unwrap();
}

fn select<'a>(out: &mut String, name: &str, label: &str, options: impl Iterator<Item = &'a str>) {
    writeln!(
        out,
        "<label>{}<select name=\"{}\"><option value=\"\">All</option>",
        html(label),
        html_attr(name)
    )
    .unwrap();
    for option in options {
        writeln!(
            out,
            "<option value=\"{}\">{}</option>",
            html_attr(option),
            html(option)
        )
        .unwrap();
    }
    writeln!(out, "</select></label>").unwrap();
}

fn status_badge(status: &str) -> String {
    if status.is_empty() {
        String::new()
    } else {
        format!(" <span class=\"btc-status\">{}</span>", html(status))
    }
}

fn icon(kind: &str) -> &'static str {
    match kind {
        "pull_request" => "⑂",
        "review" => "✓",
        "commit" => "●",
        "comment" => "↩",
        "issue" => "!",
        _ => "•",
    }
}

fn render_css() -> &'static str {
    r#".btc-page { padding-top: var(--space-lg); }
.btc-muted { color: var(--text-light); font-size: 0.92rem; }
.btc-stats { display: grid; grid-template-columns: repeat(auto-fit, minmax(7rem, 1fr)); gap: 0.5rem; padding: 0; border: 0; background: transparent; box-shadow: none; }
.btc-stats div { border: 1px solid var(--border); border-radius: var(--standard-border-radius); padding: 0.45rem 0.55rem; background: var(--panel-bg); }
.btc-stats strong { display: block; color: var(--accent-hover); font-size: 1.2rem; line-height: 1; }
.btc-stats span { color: var(--text-light); font-size: 0.78rem; text-transform: uppercase; }
.btc-filters { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)) auto; gap: 0.6rem; align-items: end; margin: var(--space-lg) 0; }
.btc-filters label { font-size: 0.78rem; text-transform: uppercase; }
.btc-filters select { width: 100%; margin: 0.2rem 0 0; }
.btc-reset { align-self: center; white-space: nowrap; }
.btc-timeline { position: relative; }
.btc-thread { margin-bottom: 0.65rem; }
.btc-thread[hidden] { display: none; }
.btc-thread summary { display: grid; grid-template-columns: 1.8rem 1fr; gap: 0.45rem; align-items: start; word-break: normal; }
.btc-thread summary small { display: block; margin-top: 0.12rem; color: var(--text-light); font-weight: normal; }
.btc-icon { display: inline-grid; place-items: center; width: 1.35rem; height: 1.35rem; border: 1px solid var(--border-soft); border-radius: 50%; color: var(--accent-hover); }
.btc-thread ol { margin: 0.45rem 0 0 1.75rem; padding-left: 1rem; }
.btc-thread li { margin: 0.28rem 0; }
.btc-thread time, .btc-kind, .btc-status { color: var(--text-light); font-size: 0.78rem; }
.btc-kind, .btc-status { border: 1px solid var(--border-soft); border-radius: var(--standard-border-radius); padding: 0.03rem 0.28rem; }
@media only screen and (max-width: 720px) {
  .btc-filters { grid-template-columns: 1fr; }
  .btc-thread summary { grid-template-columns: 1.5rem 1fr; }
}
"#
}

fn render_js() -> &'static str {
    r#"(function () {
  const form = document.getElementById("btc-filters");
  const items = Array.from(document.querySelectorAll(".btc-thread"));
  if (!form) return;
  function apply() {
    const data = new FormData(form);
    const repo = data.get("repo");
    const type = data.get("type");
    const year = data.get("year");
    for (const item of items) {
      const ok = (!repo || item.dataset.repo === repo) &&
        (!type || item.dataset.type === type || item.querySelector(`[data-type="${CSS.escape(type)}"]`)) &&
        (!year || item.dataset.year === year);
      item.hidden = !ok;
    }
  }
  form.addEventListener("change", apply);
})();
"#
}

fn parse_json(input: &str) -> Result<Json> {
    let mut p = Parser {
        chars: input.chars().collect(),
        pos: 0,
    };
    let value = p.value()?;
    p.ws();
    if p.pos != p.chars.len() {
        return Err("trailing JSON input".into());
    }
    Ok(value)
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
}

impl Parser {
    fn value(&mut self) -> Result<Json> {
        self.ws();
        match self.peek() {
            Some('n') => self.literal("null", Json::Null),
            Some('t') => self.literal("true", Json::Bool(())),
            Some('f') => self.literal("false", Json::Bool(())),
            Some('"') => self.string().map(Json::String),
            Some('[') => self.array(),
            Some('{') => self.object(),
            Some('-' | '0'..='9') => self.number(),
            _ => Err("invalid JSON value".into()),
        }
    }

    fn literal(&mut self, lit: &str, value: Json) -> Result<Json> {
        for expected in lit.chars() {
            if self.bump() != Some(expected) {
                return Err(format!("expected {lit}"));
            }
        }
        Ok(value)
    }

    fn string(&mut self) -> Result<String> {
        self.expect('"')?;
        let mut out = String::new();
        while let Some(ch) = self.bump() {
            match ch {
                '"' => return Ok(out),
                '\\' => match self
                    .bump()
                    .ok_or_else(|| "unterminated escape".to_string())?
                {
                    '"' => out.push('"'),
                    '\\' => out.push('\\'),
                    '/' => out.push('/'),
                    'b' => out.push('\u{0008}'),
                    'f' => out.push('\u{000c}'),
                    'n' => out.push('\n'),
                    'r' => out.push('\r'),
                    't' => out.push('\t'),
                    'u' => {
                        let mut code = 0u32;
                        for _ in 0..4 {
                            code = code * 16
                                + self
                                    .bump()
                                    .and_then(|c| c.to_digit(16))
                                    .ok_or_else(|| "invalid unicode escape".to_string())?;
                        }
                        if let Some(c) = char::from_u32(code) {
                            out.push(c);
                        }
                    }
                    other => return Err(format!("invalid escape {other}")),
                },
                other => out.push(other),
            }
        }
        Err("unterminated string".into())
    }

    fn array(&mut self) -> Result<Json> {
        self.expect('[')?;
        let mut out = Vec::new();
        loop {
            self.ws();
            if self.peek() == Some(']') {
                self.bump();
                return Ok(Json::Array(out));
            }
            out.push(self.value()?);
            self.ws();
            match self.bump() {
                Some(',') => {}
                Some(']') => return Ok(Json::Array(out)),
                _ => return Err("expected ',' or ']'".into()),
            }
        }
    }

    fn object(&mut self) -> Result<Json> {
        self.expect('{')?;
        let mut out = BTreeMap::new();
        loop {
            self.ws();
            if self.peek() == Some('}') {
                self.bump();
                return Ok(Json::Object(out));
            }
            let key = self.string()?;
            self.ws();
            self.expect(':')?;
            out.insert(key, self.value()?);
            self.ws();
            match self.bump() {
                Some(',') => {}
                Some('}') => return Ok(Json::Object(out)),
                _ => return Err("expected ',' or '}'".into()),
            }
        }
    }

    fn number(&mut self) -> Result<Json> {
        let start = self.pos;
        while matches!(self.peek(), Some('-' | '+' | '.' | 'e' | 'E' | '0'..='9')) {
            self.bump();
        }
        let raw: String = self.chars[start..self.pos].iter().collect();
        Ok(Json::Number(
            raw.parse().map_err(|_| format!("invalid number {raw}"))?,
        ))
    }

    fn ws(&mut self) {
        while matches!(self.peek(), Some(' ' | '\n' | '\r' | '\t')) {
            self.bump();
        }
    }

    fn expect(&mut self, ch: char) -> Result<()> {
        (self.bump() == Some(ch))
            .then_some(())
            .ok_or_else(|| format!("expected {ch}"))
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.pos += 1;
        Some(ch)
    }
}

impl Json {
    fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(map) => map.get(key),
            _ => None,
        }
    }

    fn get_path(&self, path: &[&str]) -> Option<&Json> {
        let mut cur = self;
        for key in path {
            cur = cur.get(key)?;
        }
        Some(cur)
    }

    fn string(&self) -> Option<&str> {
        match self {
            Json::String(s) => Some(s),
            _ => None,
        }
    }

    fn number(&self) -> Option<f64> {
        match self {
            Json::Number(n) => Some(*n),
            _ => None,
        }
    }

    fn array(&self) -> Option<&[Json]> {
        match self {
            Json::Array(v) => Some(v),
            _ => None,
        }
    }
}

fn write_parented(path: &Path, body: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(path, body).map_err(|e| format!("{}: {e}", path.display()))
}

fn json_escape(s: &str) -> String {
    s.chars()
        .flat_map(|c| match c {
            '"' => "\\\"".chars().collect::<Vec<_>>(),
            '\\' => "\\\\".chars().collect(),
            '\n' => "\\n".chars().collect(),
            '\r' => "\\r".chars().collect(),
            '\t' => "\\t".chars().collect(),
            c if c.is_control() => format!("\\u{:04x}", c as u32).chars().collect(),
            c => vec![c],
        })
        .collect()
}

fn unescape_json_string(s: &str) -> String {
    s.replace("\\\"", "\"")
        .replace("\\n", "\n")
        .replace("\\\\", "\\")
}

fn html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn html_attr(s: &str) -> String {
    html(s).replace('"', "&quot;")
}

fn now_rfc3339() -> String {
    unix_to_rfc3339(now_secs())
}

fn months_ago_rfc3339(months: i64) -> String {
    unix_to_rfc3339(now_secs() - months * 31 * 24 * 60 * 60)
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn unix_to_rfc3339(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = rem / 3600;
    let minute = (rem % 3600) / 60;
    let second = rem % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    (y + (m <= 2) as i64, m, d)
}

fn short_date(iso: &str) -> String {
    iso.get(0..10).unwrap_or(iso).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_config_arrays() {
        let items = parse_toml_array(r#"["bitcoin", "hwi"]"#).unwrap();
        assert_eq!(items, vec!["bitcoin", "hwi"]);
    }

    #[test]
    fn merges_by_event_id() {
        let mut existing = vec![Event {
            id: "a".into(),
            event_type: "issue".into(),
            repo: "o/r".into(),
            title: "old".into(),
            url: "u".into(),
            occurred_at: "2026-01-01T00:00:00Z".into(),
            thread_id: "t".into(),
            thread_title: "t".into(),
            thread_url: "u".into(),
            status: String::new(),
        }];
        let mut replacement = existing[0].clone();
        replacement.title = "new".into();
        merge_events(&mut existing, vec![replacement]);
        assert_eq!(existing.len(), 1);
        assert_eq!(existing[0].title, "new");
    }

    #[test]
    fn feed_round_trip_fixture() {
        let feed = Feed::from_json_file(Path::new("fixtures/feed.json")).unwrap();
        assert_eq!(feed.events.len(), 2);
        let reparsed = parse_json(&feed.to_json()).unwrap();
        assert_eq!(
            reparsed.get("username").and_then(Json::string),
            Some("trevarj")
        );
    }

    #[test]
    fn renders_static_page() {
        let cfg = Config {
            username: "trevarj".into(),
            title: "Bitcoin FOSS Contributions".into(),
            base_path: "/btc_foss/".into(),
            site_root: "https://trevs.site".into(),
            bootstrap_months: 5,
            allowlist: BTreeSet::new(),
            keywords: vec!["bitcoin".into()],
            exclude: BTreeSet::new(),
        };
        let feed = Feed::from_json_file(Path::new("fixtures/feed.json")).unwrap();
        let html = render_html(&cfg, &feed);
        assert!(html.contains("feed.json"));
        assert!(html.contains("wizardsardine/bhwi"));
    }
}
