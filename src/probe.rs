/// Known service kinds that logslim has built-in awareness of.
#[derive(Debug, Clone, PartialEq)]
pub enum ServiceKind {
    NiFi,
    Kafka,
    ClickHouse,
    Kubernetes,
    Redis,
    Docker,
    Python,
    MongoDB,
    Generic,
}

/// Log format hints.
#[derive(Debug, Clone, PartialEq)]
pub enum LogFormat {
    Log4j,    // 2024-01-01 10:00:00,123 LEVEL [thread] class - message
    Logback,  // 10:00:00.123 [thread] LEVEL class - message
    Json,     // {"timestamp":...}
    Syslog,   // Jan 01 10:00:00 host process[pid]: message
    Plain,    // no detectable structure
}

pub struct ProbeResult {
    pub service: ServiceKind,
    pub format: LogFormat,
}

/// Sniff up to 200 lines to detect the log format and service.
pub fn probe(lines: &[String]) -> ProbeResult {
    let sample = &lines[..lines.len().min(200)];
    let format = detect_format(sample);
    let service = detect_service(sample);
    ProbeResult { service, format }
}

fn detect_format(lines: &[String]) -> LogFormat {
    let mut log4j = 0usize;
    let mut json = 0usize;
    let mut syslog = 0usize;

    for line in lines {
        if line.starts_with('{') && line.ends_with('}') {
            json += 1;
        } else if is_log4j_line(line) {
            log4j += 1;
        } else if is_syslog_line(line) {
            syslog += 1;
        }
    }

    let total = lines.len().max(1);
    if json * 3 > total {
        return LogFormat::Json;
    }
    if log4j * 2 > total {
        return LogFormat::Log4j;
    }
    if syslog * 2 > total {
        return LogFormat::Syslog;
    }
    LogFormat::Plain
}

fn is_log4j_line(line: &str) -> bool {
    // Pattern: YYYY-MM-DD HH:MM:SS or YYYY-MM-DDTHH:MM:SS at the start
    let b = line.as_bytes();
    if b.len() < 19 {
        return false;
    }
    b[4] == b'-' && b[7] == b'-' && (b[10] == b' ' || b[10] == b'T') && b[13] == b':' && b[16] == b':'
}

fn is_syslog_line(line: &str) -> bool {
    // Pattern: Mon DD HH:MM:SS
    let months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    months.iter().any(|m| line.starts_with(m))
}

fn detect_service(lines: &[String]) -> ServiceKind {
    let combined: String = lines.iter().take(50).map(|s| s.as_str()).collect::<Vec<_>>().join("\n");

    if combined.contains("o.a.nifi") || combined.contains("NiFi") || combined.contains("FlowController") || combined.contains("WriteAheadFlowFileRepository") {
        return ServiceKind::NiFi;
    }
    if combined.contains("kafka") || combined.contains("Kafka") || combined.contains("KafkaController") || combined.contains("[Producer clientId") {
        return ServiceKind::Kafka;
    }
    if combined.contains("ClickHouse") || combined.contains("clickhouse") {
        return ServiceKind::ClickHouse;
    }
    if combined.contains("kubernetes") || combined.contains("kubelet") || combined.contains("kube-proxy") || combined.contains("k8s.io") {
        return ServiceKind::Kubernetes;
    }
    if combined.contains("redis") || combined.contains("Redis") || combined.contains("# Server") {
        return ServiceKind::Redis;
    }
    if combined.contains("mongod") || combined.contains("MongoDB") || combined.contains("NETWORK  [conn") {
        return ServiceKind::MongoDB;
    }
    if combined.contains("\"level\"") || combined.contains("\"msg\"") || combined.contains("\"message\"") {
        return ServiceKind::Python;
    }
    ServiceKind::Generic
}
