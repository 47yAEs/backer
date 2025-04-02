use crate::{BackerError, OutputFormat, Result, ScanResult};
use chrono::Local;
use log::{info, debug};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::Path;
use url::Url;
use rand::seq::SliceRandom;
use rand::thread_rng;
use reqwest::Client;
use std::time::Duration;
use crate::patterns::PatternGenerator;

/// 加载并处理目标站点列表
pub async fn load_targets<P: AsRef<Path>>(path: P) -> Result<Vec<String>> {
    // 不输出加载信息
    
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    
    let mut unique_targets = HashSet::new();
    
    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();
        
        if !trimmed.is_empty() && !trimmed.starts_with('#') {
            // 检测并修正URL协议
            let url = detect_url_protocol(trimmed).await?;
            unique_targets.insert(url);
        }
    }
    
    let targets: Vec<String> = unique_targets.into_iter().collect();
    
    Ok(targets)
}

/// 加载自定义备份文件模式
pub fn load_patterns<P: AsRef<Path>>(path: P) -> Result<Vec<String>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    
    let mut patterns = Vec::new();
    
    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();
        
        if !trimmed.is_empty() && !trimmed.starts_with('#') {
            patterns.push(trimmed.to_string());
        }
    }
    
    if patterns.is_empty() {
        // 如果加载的模式为空，使用默认模式
        patterns = get_default_patterns();
    }
    
    Ok(patterns)
}

/// 获取默认的备份文件模式
fn get_default_patterns() -> Vec<String> {
    vec![
        "backup.zip".to_string(),
        "backup.tar.gz".to_string(),
        "backup.sql".to_string(),
        "backup.bak".to_string(),
        "www.zip".to_string(),
        "www.tar.gz".to_string(),
        "site.zip".to_string(),
        "site.tar.gz".to_string(),
        "website.zip".to_string(),
        "db.sql".to_string(),
        "database.sql".to_string(),
        "data.sql".to_string(),
        "dump.sql".to_string(),
        "backup.old".to_string(),
        "backup.rar".to_string(),
        "db.zip".to_string(),
        "db.tar.gz".to_string(),
    ]
}

/// 为目标站点生成备份文件URL列表
pub fn generate_backup_urls(target: &str, patterns: &[String]) -> Vec<String> {
    // 使用PatternGenerator生成更完整的URL列表
    let mut generator = PatternGenerator::new();
    
    // 将patterns添加到generator中
    for pattern in patterns {
        if pattern.starts_with('.') {
            generator.full_paths.push(pattern.clone());
        } else {
            generator.prefixes.push(pattern.clone());
        }
    }
    
    // 生成URL列表
    match generator.generate_urls(target) {
        Ok(urls) => urls,
        Err(e) => {
            // 生成失败时，使用更简单的方法
            log::warn!("使用PatternGenerator生成URL失败: {:?}，回退到简单方法", e);
            generate_simple_backup_urls(target, patterns)
        }
    }
}

/// 使用简单方法生成备份文件URL列表（回退方案）
fn generate_simple_backup_urls(target: &str, patterns: &[String]) -> Vec<String> {
    let mut urls = Vec::new();
    
    // 解析基础URL
    if let Ok(parsed_url) = Url::parse(target) {
        let base_url = format!("{}://{}", parsed_url.scheme(), parsed_url.host_str().unwrap_or(""));
        
        // 直接在根目录下应用模式
        for pattern in patterns {
            urls.push(format!("{}/{}", base_url, pattern));
        }
        
        // 常见的备份目录
        let backup_dirs = ["backup", "bak", "old", "archive", "db", "data"];
        
        // 在备份目录下应用模式
        for dir in backup_dirs {
            for pattern in patterns {
                urls.push(format!("{}/{}/{}", base_url, dir, pattern));
            }
        }
    }
    
    urls
}

/// 规范化URL格式
pub fn normalize_url(url: &str) -> Result<String> {
    // 检查URL是否有协议前缀，如果没有则添加http://
    let url_str = if !url.starts_with("http://") && !url.starts_with("https://") {
        format!("http://{}", url)
    } else {
        url.to_string()
    };
    
    // 解析URL并确保其有效
    let parsed = match Url::parse(&url_str) {
        Ok(url) => url,
        Err(e) => return Err(BackerError::Url(e)),
    };
    
    // 删除URL中的路径、查询参数等，只保留域名部分
    let mut normalized = format!("{}://{}", parsed.scheme(), parsed.host_str().unwrap_or(""));
    if let Some(port) = parsed.port() {
        if (parsed.scheme() == "http" && port != 80) || (parsed.scheme() == "https" && port != 443) {
            normalized.push_str(&format!(":{}", port));
        }
    }
    
    Ok(normalized)
}

/// 保存扫描结果
pub fn save_results<P: AsRef<Path> + Clone>(
    results: &[ScanResult],
    format: OutputFormat,
    path: Option<P>,
) -> Result<()> {
    if results.is_empty() {
        info!("没有发现任何备份文件");
        return Ok(());
    }
    
    info!("发现 {} 个潜在的备份文件", results.len());
    
    if let Some(path) = path {
        match format {
            OutputFormat::Json => save_json(results, path.clone())?,
            OutputFormat::Csv => save_csv(results, path.clone())?,
            OutputFormat::Markdown => save_markdown(results, path.clone())?,
        }
        
        println!("结果已保存到 {}", path.as_ref().display());
    } else {
        // 如果没有指定输出文件，打印到控制台
        for result in results {
            println!("URL: {}, 状态码: {}, 内容类型: {}, 内容长度: {}, 已验证: {}", 
                result.url, 
                result.status_code, 
                result.content_type.as_deref().unwrap_or("未知"), 
                result.content_length.map_or("未知".to_string(), |len| len.to_string()),
                result.verified
            );
        }
    }
    
    Ok(())
}

/// 将结果保存为JSON格式
fn save_json<P: AsRef<Path>>(results: &[ScanResult], path: P) -> Result<()> {
    let json = serde_json::to_string_pretty(results)?;
    fs::write(path, json)?;
    Ok(())
}

/// 将结果保存为CSV格式
fn save_csv<P: AsRef<Path>>(results: &[ScanResult], path: P) -> Result<()> {
    let mut writer = csv::Writer::from_path(path)?;
    
    writer.write_record(&["URL", "状态码", "内容类型", "内容长度", "已验证"])?;
    
    for result in results {
        writer.write_record(&[
            &result.url,
            &result.status_code.to_string(),
            &result.content_type.clone().unwrap_or_else(|| "未知".to_string()),
            &result.content_length.map_or("未知".to_string(), |len| len.to_string()),
            &result.verified.to_string(),
        ])?;
    }
    
    writer.flush()?;
    Ok(())
}

/// 将结果保存为Markdown格式
fn save_markdown<P: AsRef<Path>>(results: &[ScanResult], path: P) -> Result<()> {
    let mut markdown = String::new();
    
    // 添加标题和日期
    let now = Local::now();
    markdown.push_str(&format!("# 备份文件扫描结果\n\n"));
    markdown.push_str(&format!("扫描时间: {}\n\n", now.format("%Y-%m-%d %H:%M:%S")));
    
    // 添加表格头
    markdown.push_str("| URL | 状态码 | 内容类型 | 内容长度 | 已验证 |\n");
    markdown.push_str("|-----|--------|----------|----------|---------|\n");
    
    // 添加结果行
    for result in results {
        markdown.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            result.url,
            result.status_code,
            result.content_type.as_deref().unwrap_or("未知"),
            result.content_length.map_or("未知".to_string(), |len| len.to_string()),
            if result.verified { "✅" } else { "❌" }
        ));
    }
    
    fs::write(path, markdown)?;
    Ok(())
}

/// 分析多个URL，提取其共同的根域名
pub fn extract_common_root_domain(urls: &[String]) -> Option<String> {
    if urls.is_empty() {
        return None;
    }
    
    // 尝试提取每个URL的根域名
    let mut domains = HashMap::new();
    for url_str in urls {
        if let Ok(url) = Url::parse(url_str) {
            if let Some(host) = url.host_str() {
                let parts: Vec<&str> = host.split('.').collect();
                if parts.len() >= 2 {
                    // 提取根域名（最后两部分）
                    let root_domain = format!("{}.{}", parts[parts.len() - 2], parts[parts.len() - 1]);
                    *domains.entry(root_domain).or_insert(0) += 1;
                }
            }
        }
    }
    
    // 找出出现次数最多的根域名
    domains.into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(domain, _)| domain)
}

/// 生成随机User-Agent
pub fn get_random_user_agent() -> String {
    let user_agents = vec![
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36",
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/92.0.4515.159 Safari/537.36",
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/15.0 Safari/605.1.15",
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:90.0) Gecko/20100101 Firefox/90.0",
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.114 Safari/537.36",
        "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.101 Safari/537.36",
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36 Edg/91.0.864.59",
        "Mozilla/5.0 (iPhone; CPU iPhone OS 14_6 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/14.0 Mobile/15E148 Safari/604.1",
        "Mozilla/5.0 (iPad; CPU OS 14_6 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/14.0 Mobile/15E148 Safari/604.1",
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/92.0.4515.107 Safari/537.36 OPR/78.0.4093.112",
    ];
    
    user_agents.choose(&mut thread_rng())
        .unwrap_or(&"Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36")
        .to_string()
}

/// 自动检测URL协议(http/https)
pub async fn detect_url_protocol(input: &str) -> Result<String> {
    // 如果已经包含协议，直接返回
    if input.starts_with("http://") || input.starts_with("https://") {
        return Ok(input.to_string());
    }

    // 移除可能的前缀www.和末尾的斜杠
    let domain = input.trim().trim_start_matches("www.").trim_end_matches('/');
    
    debug!("尝试检测域名协议: {}", domain);
    
    // 创建一个临时客户端用于探测，更短的超时
    let client = Client::builder()
        .timeout(Duration::from_secs(3)) // 更短的超时
        .use_rustls_tls() // 使用rustls提高性能
        .build()?;
    
    // 首先尝试HTTPS
    let https_url = format!("https://{}", domain);
    
    // 使用异步超时和HEAD请求，仅检查连接性
    let https_result = tokio::time::timeout(
        Duration::from_secs(3), 
        client.head(&https_url)
            .header("User-Agent", get_random_user_agent())
            .send()
    ).await;
    
    match https_result {
        Ok(Ok(response)) => {
            let status = response.status().as_u16();
            // 接受任何响应，只要能连接，包括错误状态码
            debug!("HTTPS连接成功: {} 状态码: {}", https_url, status);
            return Ok(https_url);
        },
        _ => {
            debug!("HTTPS连接失败，尝试HTTP");
        }
    };
    
    // 如果HTTPS失败，尝试HTTP
    let http_url = format!("http://{}", domain);
    
    // 使用异步超时
    let http_result = tokio::time::timeout(
        Duration::from_secs(3),
        client.head(&http_url)
            .header("User-Agent", get_random_user_agent())
            .send()
    ).await;
    
    match http_result {
        Ok(Ok(response)) => {
            let status = response.status().as_u16();
            debug!("HTTP连接成功: {} 状态码: {}", http_url, status);
            return Ok(http_url);
        },
        _ => {
            debug!("HTTP连接也失败，默认使用HTTP");
        }
    };
    
    // 两种协议都失败，默认使用http版本
    debug!("默认使用HTTP: {}", http_url);
    Ok(http_url)
}
