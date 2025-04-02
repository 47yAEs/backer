use crate::{Result, ScanResult};
use crate::utils::get_random_user_agent;
use log::{debug, warn};
use rand::prelude::*;
use rand::seq::SliceRandom;
use reqwest::{Client, header::{HeaderMap, HeaderValue, USER_AGENT, HeaderName}, StatusCode};
use std::str::FromStr;
use std::time::{Duration, Instant};
use tokio::time::timeout;
use std::collections::HashMap;
use url::Url;
use std::sync::{Arc, Mutex};

/// HTTP客户端包装器
#[derive(Clone)]
pub struct HttpClient {
    client: Client,
    timeout_secs: u64,
    #[allow(dead_code)]
    retry_count: u32,
    user_agent: String,
    random_headers: bool,
    random_ip: bool,
    // 域名响应时间跟踪
    #[allow(dead_code)]
    response_times: Arc<Mutex<HashMap<String, Vec<Duration>>>>,
    // 连接预热状态
    warmed_up_hosts: Arc<Mutex<HashMap<String, bool>>>,
    // 429/503响应计数
    #[allow(dead_code)]
    rate_limited_hosts: Arc<Mutex<HashMap<String, (usize, Instant)>>>,
    // 请求节流控制
    #[allow(dead_code)]
    throttle_factor: Arc<Mutex<f32>>,
    debug: bool,
    // 自定义User-Agent列表
    custom_user_agents: Vec<String>,
}

#[allow(dead_code)]
impl HttpClient {
    /// 创建新的HTTP客户端
    pub fn new(timeout_secs: u64, retry_count: u32, user_agent: String) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            // 启用TLS和连接池
            .use_rustls_tls()
            // 启用连接池
            .pool_idle_timeout(Duration::from_secs(90))
            .pool_max_idle_per_host(10)
            .build()?;
            
        // 预定义一些现代浏览器的User-Agent
        let default_user_agents = vec![
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/116.0.0.0 Safari/537.36".to_string(),
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:109.0) Gecko/20100101 Firefox/117.0".to_string(),
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/16.5 Safari/605.1.15".to_string(),
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/116.0.0.0 Safari/537.36 Edg/116.0.1938.69".to_string(),
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/115.0.0.0 Safari/537.36".to_string(),
            "Mozilla/5.0 (iPhone; CPU iPhone OS 16_6 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/16.0 Mobile/15E148 Safari/604.1".to_string(),
            "Mozilla/5.0 (iPad; CPU OS 16_6 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/16.0 Mobile/15E148 Safari/604.1".to_string(),
            "Mozilla/5.0 (Linux; Android 13; SM-S901B) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/116.0.0.0 Mobile Safari/537.36".to_string(),
            "Mozilla/5.0 (Windows NT 10.0; WOW64; Trident/7.0; rv:11.0) like Gecko".to_string(),
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/116.0.0.0 Safari/537.36 OPR/102.0.0.0".to_string(),
        ];
            
        Ok(Self {
            client,
            timeout_secs,
            retry_count,
            user_agent,
            random_headers: true, // 默认开启随机请求头
            random_ip: true,      // 默认开启随机IP
            response_times: Arc::new(Mutex::new(HashMap::new())),
            warmed_up_hosts: Arc::new(Mutex::new(HashMap::new())),
            rate_limited_hosts: Arc::new(Mutex::new(HashMap::new())),
            throttle_factor: Arc::new(Mutex::new(1.0)),
            debug: false,
            custom_user_agents: default_user_agents,
        })
    }
    
    /// 设置是否使用随机请求头
    pub fn set_random_headers(&mut self, enable: bool) {
        self.random_headers = enable;
    }
    
    /// 设置是否使用随机IP
    pub fn set_random_ip(&mut self, enable: bool) {
        self.random_ip = enable;
    }
    
    /// 设置是否启用调试输出
    pub fn set_debug(&mut self, enable: bool) {
        self.debug = enable;
    }
    
    /// 设置自定义User-Agent列表
    pub fn set_custom_user_agents(&mut self, user_agents: Vec<String>) {
        self.custom_user_agents = user_agents;
    }
    
    /// 添加单个自定义User-Agent
    pub fn add_custom_user_agent(&mut self, user_agent: String) {
        self.custom_user_agents.push(user_agent);
    }
    
    /// 预热目标主机连接
    pub async fn warm_up_connection(&self, base_url: &str) -> Result<()> {
        // 尝试解析URL获取主机名
        if let Ok(url) = Url::parse(base_url) {
            if let Some(host) = url.host_str() {
                // 检查是否已经预热过
                let mut warmed_up = self.warmed_up_hosts.lock().unwrap();
                if warmed_up.contains_key(host) {
                    return Ok(());
                }
                
                // 使用更短的超时确保预热不会卡住程序
                let short_timeout = Duration::from_secs(3);
                
                // 发送HEAD请求预热连接
                let headers = self.generate_random_headers();
                
                match timeout(short_timeout, self.client.head(base_url).headers(headers).send()).await {
                    Ok(result) => {
                        if result.is_ok() {
                            // 连接成功预热
                            warmed_up.insert(host.to_string(), true);
                        }
                        // 即使失败也继续处理
                        Ok(())
                    },
                    Err(_) => {
                        // 预热超时，但不阻止继续
                        Ok(())
                    }
                }
            } else {
                Ok(())
            }
        } else {
            Ok(())
        }
    }
    
    /// 获取域名的自适应超时时间
    fn get_adaptive_timeout(&self, url_str: &str) -> Duration {
        let default_timeout = Duration::from_secs(self.timeout_secs);
        
        // 尝试解析URL获取主机名
        if let Ok(url) = Url::parse(url_str) {
            if let Some(host) = url.host_str() {
                let response_times = self.response_times.lock().unwrap();
                
                if let Some(times) = response_times.get(host) {
                    if !times.is_empty() {
                        // 计算平均响应时间
                        let avg_time: Duration = times.iter().sum::<Duration>() / times.len() as u32;
                        
                        // 自适应超时 = 平均响应时间 * 3 + 基础超时
                        let adaptive_timeout = avg_time * 3 + Duration::from_secs(2);
                        
                        // 设置上限和下限
                        if adaptive_timeout < Duration::from_secs(5) {
                            return Duration::from_secs(5);
                        } else if adaptive_timeout > default_timeout {
                            return default_timeout;
                        } else {
                            return adaptive_timeout;
                        }
                    }
                }
            }
        }
        
        // 默认超时
        default_timeout
    }
    
    /// 记录域名响应时间
    fn record_response_time(&self, url_str: &str, duration: Duration) {
        if let Ok(url) = Url::parse(url_str) {
            if let Some(host) = url.host_str() {
                let mut response_times = self.response_times.lock().unwrap();
                
                let times = response_times.entry(host.to_string()).or_insert_with(Vec::new);
                times.push(duration);
                
                // 只保留最近10次的响应时间
                if times.len() > 10 {
                    times.remove(0);
                }
            }
        }
    }
    
    /// 检查并更新请求节流状态
    fn check_rate_limiting(&self, url_str: &str, status: StatusCode) -> bool {
        if status == StatusCode::TOO_MANY_REQUESTS || status == StatusCode::SERVICE_UNAVAILABLE {
            if let Ok(url) = Url::parse(url_str) {
                if let Some(host) = url.host_str() {
                    let mut rate_limited = self.rate_limited_hosts.lock().unwrap();
                    
                    // 当前时间
                    let now = Instant::now();
                    
                    // 增加计数或插入新记录
                    let entry = rate_limited.entry(host.to_string()).or_insert((0, now));
                    entry.0 += 1;
                    entry.1 = now;
                    
                    // 如果短时间内多次被限制，增加节流因子
                    if entry.0 >= 3 {
                        let mut throttle_factor = self.throttle_factor.lock().unwrap();
                        *throttle_factor = (*throttle_factor * 1.5).min(5.0);
                        return true;
                    }
                }
            }
        }
        
        // 检查是否可以降低节流因子
        let mut can_decrease = true;
        {
            let rate_limited = self.rate_limited_hosts.lock().unwrap();
            let now = Instant::now();
            
            // 检查最近5分钟内是否有被限制
            for (_, (_, time)) in rate_limited.iter() {
                if now.duration_since(*time) < Duration::from_secs(300) {
                    can_decrease = false;
                    break;
                }
            }
        }
        
        // 如果5分钟内没有被限制，逐渐恢复节流因子
        if can_decrease {
            let mut throttle_factor = self.throttle_factor.lock().unwrap();
            if *throttle_factor > 1.0 {
                *throttle_factor = (*throttle_factor * 0.9).max(1.0);
            }
        }
        
        false
    }
    
    /// 获取当前节流延迟
    fn get_throttle_delay(&self) -> Duration {
        let factor = *self.throttle_factor.lock().unwrap();
        
        // 降低初始延迟值，从100ms降至30ms
        Duration::from_millis((30.0 * factor) as u64)
    }
    
    /// 检查URL是否可能是备份文件
    pub async fn check_url(&self, url: &str, verify_content: bool) -> Result<Option<ScanResult>> {
        // 直接做一次请求，不进行预热或多次重试
        debug!("检查URL: {}", url);
        
        // 使用更短的超时时间
        let short_timeout = std::cmp::min(self.timeout_secs, 5); // 最多5秒
        
        // 只尝试一次请求
        let request_result = timeout(
            Duration::from_secs(short_timeout),
            self.make_request(url, verify_content)
        ).await;
        
        match request_result {
            Ok(result) => result,
            Err(_) => {
                debug!("请求超时: {}", url);
                Ok(None)
            }
        }
    }
    
    /// 检查目录是否存在并返回状态码
    pub async fn check_directory(&self, url: &str) -> Result<Option<u16>> {
        debug!("检查目录状态: {}", url);
        
        // 生成随机请求头
        let headers = self.generate_random_headers();
        
        // 设置超时
        let future = self.client.get(url)
            .headers(headers.clone())
            .send();
            
        let timeout_duration = Duration::from_secs(self.timeout_secs);
        let response = match timeout(timeout_duration, future).await {
            Ok(result) => result?,
            Err(_) => {
                warn!("请求 {} 超时", url);
                return Ok(None);
            }
        };
        
        let status = response.status();
        
        // 返回状态码
        Ok(Some(status.as_u16()))
    }
    
    /// 生成随机请求头
    fn generate_random_headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        let mut rng = rand::thread_rng();
        
        // 设置User-Agent
        let user_agent = if self.user_agent.is_empty() {
            get_random_user_agent()
        } else if self.random_headers {
            // 随机UA
            self.custom_user_agents.choose(&mut rng).cloned().unwrap_or_else(get_random_user_agent)
        } else {
            // 使用指定的User-Agent
            self.user_agent.clone()
        };
        
        // 创建HeaderValue，处理错误情况
        if let Ok(header_value) = HeaderValue::from_str(&user_agent) {
            headers.insert(USER_AGENT, header_value);
        }
        
        // 添加其他随机请求头
        if self.random_headers {
            // 添加其他常见请求头
            let accept_headers = [
                ("accept", "text/html,application/xhtml+xml,application/xml;q=0.9,image/webp,*/*;q=0.8"),
                ("accept-language", "en-US,en;q=0.9,zh-CN;q=0.8,zh;q=0.7"),
                ("accept-encoding", "gzip, deflate, br"),
                ("connection", "keep-alive"),
                ("upgrade-insecure-requests", "1"),
                ("pragma", "no-cache"),
                ("cache-control", "no-cache"),
            ];
            
            for (name, value) in accept_headers {
                if rng.gen_bool(0.8) { // 80%的概率添加这个头
                    if let Ok(header_value) = HeaderValue::from_str(value) {
                        // 使用HeaderName::from_str需要导入FromStr trait
                        if let Ok(header_name) = HeaderName::from_str(name) {
                            headers.insert(header_name, header_value);
                        }
                    }
                }
            }
        }
        
        // 随机X-Forwarded-For IP
        if self.random_ip {
            let ip = format!(
                "{}.{}.{}.{}", 
                rng.gen_range(1..=254), 
                rng.gen_range(1..=254),
                rng.gen_range(1..=254),
                rng.gen_range(1..=254)
            );
            
            if let Ok(header_value) = HeaderValue::from_str(&ip) {
                // 使用HeaderName::from_str需要导入FromStr trait
                if let Ok(header_name) = HeaderName::from_str("x-forwarded-for") {
                    headers.insert(header_name, header_value);
                }
            }
        }
        
        headers
    }
    
    /// 执行HTTP请求并分析响应
    async fn make_request(&self, url: &str, verify_content: bool) -> Result<Option<ScanResult>> {
        // 生成随机请求头
        let headers = self.generate_random_headers();
        
        // 使用固定超时，避免复杂计算
        let timeout_duration = Duration::from_secs(3); // 固定3秒，比check_url更短
        
        // 开始计时
        let start_time = Instant::now();
        
        // 设置超时 - 使用HEAD请求快速检测
        let future = self.client.head(url)
            .headers(headers.clone())
            .timeout(timeout_duration) // 设置请求自身的超时
            .send();
        
        let response = match timeout(timeout_duration, future).await {
            Ok(result) => match result {
                Ok(resp) => resp,
                Err(e) => {
                    debug!("HTTP请求错误: {} - {:?}", url, e);
                    return Ok(None);
                }
            },
            Err(_) => {
                debug!("HTTP请求超时: {}", url);
                return Ok(None);
            }
        };
        
        let status = response.status();
        let duration = start_time.elapsed();
        
        // 只在调试模式下输出所有状态
        if self.debug || status.is_success() || status == StatusCode::FORBIDDEN {
            debug!("URL {} 响应状态码: {} (耗时: {:?})", url, status, duration);
        }
        
        // 【改进】备份文件判断逻辑
        // 1. 优先判断是否为200状态码（明确的成功）
        if status == StatusCode::OK {
            // 检查是否是备份文件扩展名
            if !is_backup_file_extension(url) {
                debug!("状态码为200但不是备份文件扩展名: {}", url);
                return Ok(None);
            }
            
            // 获取响应头信息
            let content_type = response.headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|h| h.to_str().ok())
                .map(String::from);
                
            let content_length = response.headers()
                .get(reqwest::header::CONTENT_LENGTH)
                .and_then(|h| h.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok());
            
            // 检查内容类型
            if let Some(ref ct) = content_type {
                // 检查是否匹配预期的备份文件类型
                if !self.is_valid_backup_content_type(ct, url) {
                    debug!("状态码为200但内容类型不匹配: {} ({})", url, ct);
                    // 我们不立即返回，因为有些服务器可能设置了错误的Content-Type
                }
            }
            
            // 检查文件大小
            if let Some(size) = content_length {
                // 排除过小的文件 (小于100字节的可能是404页面)
                if size < 100 {
                    debug!("状态码为200但文件太小: {} ({}字节)", url, size);
                    return Ok(None);
                }
                
                // 排除过大的文件，防止误报 (超过1GB)
                if size > 1_000_000_000 {
                    debug!("状态码为200但文件太大: {} ({}字节)", url, size);
                    // 我们不立即返回，因为有些备份确实很大
                }
            }
            
            // 200状态码且通过了基本校验，确认为备份文件
            debug!("确认发现备份文件 [200]: {}", url);
            return Ok(Some(ScanResult {
                url: url.to_string(),
                status_code: status.as_u16(),
                content_type,
                content_length,
                verified: verify_content,
            }));
        }
        
        // 2. 如果是403，可能是限制访问的备份文件
        else if status == StatusCode::FORBIDDEN {
            // 检查是否是备份文件扩展名
            if !is_backup_file_extension(url) {
                return Ok(None);
            }
            
            // 获取响应头信息
            let content_type = response.headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|h| h.to_str().ok())
                .map(String::from);
                
            let content_length = response.headers()
                .get(reqwest::header::CONTENT_LENGTH)
                .and_then(|h| h.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok());
            
            debug!("发现可能受限制的备份文件 [403]: {}", url);
            return Ok(Some(ScanResult {
                url: url.to_string(),
                status_code: status.as_u16(),
                content_type,
                content_length,
                verified: false, // 403状态无法验证内容
            }));
        }
        
        // 3. 其他状态码如301/302/307重定向，尝试跟随重定向
        else if status.is_redirection() {
            // 只有备份文件扩展名才尝试跟随重定向
            if !is_backup_file_extension(url) {
                return Ok(None);
            }
            
            // 获取重定向位置
            if let Some(location) = response.headers().get(reqwest::header::LOCATION) {
                if let Ok(location_str) = location.to_str() {
                    debug!("URL {} 重定向到 {}", url, location_str);
                    
                    // 尝试GET请求跟随重定向 (限制只跟随一次重定向)
                    let redirect_future = self.client.get(location_str)
                        .headers(headers)
                        .timeout(timeout_duration)
                        .send();
                        
                    match timeout(timeout_duration, redirect_future).await {
                        Ok(Ok(redirect_resp)) => {
                            let redirect_status = redirect_resp.status();
                            
                            // 如果重定向后是200，认为是备份文件
                            if redirect_status.is_success() {
                                let content_type = redirect_resp.headers()
                                    .get(reqwest::header::CONTENT_TYPE)
                                    .and_then(|h| h.to_str().ok())
                                    .map(String::from);
                                    
                                let content_length = redirect_resp.headers()
                                    .get(reqwest::header::CONTENT_LENGTH)
                                    .and_then(|h| h.to_str().ok())
                                    .and_then(|s| s.parse::<u64>().ok());
                                
                                debug!("经重定向发现备份文件: {} -> {}", url, location_str);
                                return Ok(Some(ScanResult {
                                    url: url.to_string(), // 保留原始URL
                                    status_code: redirect_status.as_u16(),
                                    content_type,
                                    content_length,
                                    verified: false,
                                }));
                            }
                        },
                        _ => {
                            debug!("跟随重定向失败: {} -> {}", url, location_str);
                        }
                    }
                }
            }
        }
        
        // 其他状态码，包括4xx和5xx，直接返回None
        Ok(None)
    }
    
    /// 检查内容类型是否符合备份文件预期
    fn is_valid_backup_content_type(&self, content_type: &str, url: &str) -> bool {
        let ct = content_type.to_lowercase();
        
        // 检查URL扩展名
        if url.ends_with(".zip") || url.ends_with(".rar") || url.ends_with(".7z") {
            return ct.contains("application/") || 
                   ct.contains("application/zip") || 
                   ct.contains("application/x-rar") || 
                   ct.contains("application/x-7z") || 
                   ct.contains("application/octet-stream");
        }
        
        if url.ends_with(".gz") || url.ends_with(".tar") || url.ends_with(".tar.gz") || url.ends_with(".tgz") {
            return ct.contains("application/") || 
                   ct.contains("application/gzip") || 
                   ct.contains("application/x-tar") || 
                   ct.contains("application/octet-stream");
        }
        
        if url.ends_with(".sql") || url.ends_with(".sql.gz") {
            return ct.contains("text/") || 
                   ct.contains("application/sql") || 
                   ct.contains("application/octet-stream");
        }
        
        if url.ends_with(".db") || url.ends_with(".sqlite") || url.ends_with(".mdb") {
            return ct.contains("application/") || 
                   ct.contains("application/octet-stream");
        }
        
        if url.ends_with(".bak") || url.contains(".backup") || url.ends_with(".old") {
            // 这些通用后缀可能是任何文件类型
            return true;
        }
        
        // 临时文件
        url.ends_with(".tmp") ||
        url.ends_with(".temp") ||
        url.ends_with(".swp") ||
        url.ends_with(".save") ||
        url.ends_with(".old.php")
    }
}

/// 检查URL是否有备份文件扩展名
fn is_backup_file_extension(url: &str) -> bool {
    let url_lower = url.to_lowercase();
    
    // 压缩文件常见格式
    url_lower.ends_with(".zip") ||
    url_lower.ends_with(".rar") ||
    url_lower.ends_with(".tar") ||
    url_lower.ends_with(".tar.gz") ||
    url_lower.ends_with(".7z") ||
    
    // 数据库备份格式
    url_lower.ends_with(".sql") ||
    url_lower.ends_with(".sql.gz") ||
    url_lower.ends_with(".sql.bz2") ||
    url_lower.ends_with(".sqlite") ||
    url_lower.ends_with(".sqlite3") ||
    url_lower.ends_with(".db") ||
    url_lower.ends_with(".mdb") ||
    url_lower.ends_with(".dump") ||
    
    // 常见备份后缀
    url_lower.ends_with(".bak") ||
    url_lower.ends_with(".old") ||
    url_lower.ends_with(".backup") ||
    url_lower.ends_with(".back") ||
    url_lower.ends_with("_backup") ||
    url_lower.ends_with("-backup") ||
    url_lower.ends_with(".copy") ||
    url_lower.ends_with(".orig") ||
    url_lower.ends_with(".original") ||
    url_lower.ends_with(".txt") ||
    
    // 敏感文件
    url_lower.ends_with("/.git/config") ||
    url_lower.ends_with("/.git/HEAD") ||
    url_lower.ends_with("/.svn/entries") ||
    url_lower.ends_with("/.env") ||
    url_lower.ends_with("/.htpasswd") ||
    url_lower.ends_with("/wp-config.php.bak") ||
    url_lower.ends_with("/config.php.bak") ||
    url_lower.contains(".config.") ||
    url_lower.contains("/.git/") ||
    url_lower.contains("/.svn/") ||
    
    // 临时文件
    url_lower.ends_with(".tmp") ||
    url_lower.ends_with(".temp") ||
    url_lower.ends_with(".swp") ||
    url_lower.ends_with(".save") ||
    url_lower.ends_with(".old.php")
}