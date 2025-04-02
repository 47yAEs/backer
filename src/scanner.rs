use crate::{Result, ScanConfig, ScanResult};
use crate::http::HttpClient;
use crate::utils::generate_backup_urls;
use futures::future;
use indicatif::{ProgressBar, ProgressStyle};
use log::debug;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::Semaphore;
use std::time::{Instant, Duration};
use std::fs::File;
use std::io::Write;
use serde_json;

/// 扫描器核心
pub struct Scanner {
    config: ScanConfig,
    client: HttpClient,
    // 模式成功率追踪
    pattern_success_rates: Arc<Mutex<HashMap<String, (usize, usize)>>>, // (成功数, 总尝试数)
    // 当前动态线程数
    current_threads: Arc<Mutex<usize>>,
}

#[allow(dead_code)]
impl Scanner {
    /// 创建新的扫描器
    pub async fn new(config: ScanConfig) -> Result<Self> {
        let client = HttpClient::new(
            config.timeout,
            config.retry_count,
            config.user_agent.clone(),
        )?;

        // 复制线程数
        let threads = config.threads;
        
        Ok(Self {
            config: config.clone(),
            client,
            pattern_success_rates: Arc::new(Mutex::new(HashMap::new())),
            current_threads: Arc::new(Mutex::new(threads)),
        })
    }
    
    /// 设置是否使用随机请求头
    pub fn set_random_headers(&mut self, enable: bool) {
        self.client.set_random_headers(enable);
    }
    
    /// 设置是否使用随机IP
    pub fn set_random_ip(&mut self, enable: bool) {
        self.client.set_random_ip(enable);
    }
    
    /// 设置debug模式
    pub fn set_debug(&mut self, enable: bool) {
        self.client.set_debug(enable);
    }
    
    /// 扫描目标站点
    pub async fn scan(&mut self, targets: Vec<String>) -> Result<Vec<ScanResult>> {
        let mut all_results = Vec::new();
        
        // 创建进度条，修改为用户需要的样式
        let progress_bar = ProgressBar::new(targets.len() as u64)
            .with_style(ProgressStyle::default_bar()
                .template("{msg} [{elapsed_precise}] [{bar:50}] {pos}/{len} ({eta})")
                .unwrap()
                .progress_chars("=>")); // 使用"=>"，这会显示为[==============>    ]
        
        progress_bar.set_message("目标处理");
        
        // 加载备份文件模式
        let patterns = match &self.config.patterns_file {
            Some(path) => crate::utils::load_patterns(path)?,
            None => Vec::new(),
        };
        
        // 按域名分组处理，避免同时请求过多相同域名
        let mut domain_targets: HashMap<String, Vec<String>> = HashMap::new();
        
        for target in targets {
            // 简单提取域名 (更复杂的实现可以使用url crate)
            let domain = match target.split("://").nth(1) {
                Some(d) => match d.split('/').next() {
                    Some(domain) => domain.to_string(),
                    None => target.clone(),
                },
                None => target.clone(),
            };
            
            domain_targets.entry(domain).or_insert_with(Vec::new).push(target);
        }
        
        // 总任务数
        let total_domains = domain_targets.len();
        progress_bar.set_length(total_domains as u64);
        
        // 对每个域名进行处理
        for (domain, domain_targets) in domain_targets {
            progress_bar.set_message(format!("域名: {}", domain));
            
            for target in domain_targets {
                // 为每个目标生成备份文件URL
                let urls = generate_backup_urls(&target, &patterns);
                
                // 对URL模式按历史成功率排序
                let sorted_urls = self.sort_urls_by_success_rate(urls);
                
                // 扫描URL
                let results = self.scan_urls(&self.client, sorted_urls, self.config.verify_content, progress_bar.clone()).await;
                
                // 合并结果
                all_results.extend(results);
            }
            
            progress_bar.inc(1);
        }
        
        progress_bar.finish();
        
        Ok(all_results)
    }
    
    /// 根据历史成功率排序URL
    fn sort_urls_by_success_rate(&self, urls: Vec<String>) -> Vec<String> {
        let success_rates = self.pattern_success_rates.lock().unwrap();
        
        // 如果没有历史数据，直接返回原始顺序
        if success_rates.is_empty() {
            return urls;
        }
        
        // 计算每个URL的得分
        let mut url_scores: Vec<(String, f64)> = urls
            .into_iter()
            .map(|url| {
                // 提取模式
                let pattern = self.extract_pattern_from_url(&url);
                
                // 计算成功率
                let score = if let Some((successes, attempts)) = success_rates.get(&pattern) {
                    if *attempts > 0 {
                        (*successes as f64) / (*attempts as f64)
                    } else {
                        0.0
                    }
                } else {
                    // 默认得分 (0.1表示新模式有一定的探索机会)
                    0.1
                };
                
                (url, score)
            })
            .collect();
        
        // 按得分排序 (降序)
        url_scores.sort_by(|(_, a), (_, b)| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
        
        // 返回排序后的URL
        url_scores.into_iter().map(|(url, _)| url).collect()
    }
    
    /// 从URL中提取模式
    fn extract_pattern_from_url(&self, url: &str) -> String {
        // 从URL中提取模式，例如从 http://example.com/backup.zip 提取 backup.zip
        
        let parts: Vec<&str> = url.split('/').collect();
        if let Some(last) = parts.last() {
            return last.to_string();
        }
        
        url.to_string()
    }
    
    /// 更新模式成功率
    fn update_pattern_success_rate(&self, url: &str, success: bool) {
        // 提取模式
        let pattern = if let Some(pattern) = url.split('/').last() {
            pattern.to_string()
        } else {
            return;
        };
        
        // 更新成功率
        let mut rates = self.pattern_success_rates.lock().unwrap();
        let entry = rates.entry(pattern).or_insert((0, 0));
        
        if success {
            entry.0 += 1;  // 成功数+1
        }
        entry.1 += 1;  // 总数+1
    }
    
    /// 将单个扫描结果保存为JSON文件
    fn save_result_to_json(&self, result: &ScanResult, path: &str) -> Result<()> {
        // 创建包含单个结果的数组
        let results = vec![result];
        
        // 序列化为JSON
        let json = serde_json::to_string_pretty(&results)?;
        
        // 写入文件
        let mut file = File::create(path)?;
        file.write_all(json.as_bytes())?;
        
        Ok(())
    }
    
    /// 动态调整线程数
    fn adjust_concurrency(&self, status_code: u16) {
        let mut current_threads = self.current_threads.lock().unwrap();
        
        // 如果遇到限制，减少线程数
        if status_code == 429 || status_code == 503 {
            *current_threads = (*current_threads * 3 / 4).max(1);
        } 
        // 如果运行平稳，可以考虑增加线程数，但不超过配置的最大值
        else if *current_threads < self.config.threads && status_code < 400 {
            *current_threads = (*current_threads * 5 / 4).min(self.config.threads);
        }
    }
    
    /// 获取当前线程数
    fn get_current_threads(&self) -> usize {
        let current_threads = self.current_threads.lock().unwrap();
        *current_threads
    }
    
    /// 扫描指定URL列表
    async fn scan_urls(&self, client: &HttpClient, urls: Vec<String>, verify_content: bool, progress_bar: ProgressBar) -> Vec<ScanResult> {
        let results = Arc::new(Mutex::new(Vec::new()));
        
        // 开始计时
        let start_time = Instant::now();
        
        // 使用固定线程数，避免动态调整造成的复杂性
        let threads = std::cmp::min(self.config.threads, 5); 
        let semaphore = Arc::new(Semaphore::new(threads));
        
        // 统计根目录URL数量（假设PatternGenerator正确将根目录URL放在前面）
        let root_url_count = std::cmp::min(urls.len(), 200); // 假设前200个是根目录URL
        let backup_urls = if urls.len() > root_url_count {
            urls[root_url_count..].to_vec()
        } else {
            Vec::new()
        };
        let root_urls = urls[0..root_url_count].to_vec();
        
        // 设置根目录进度条
        progress_bar.set_length(root_url_count as u64);
        progress_bar.set_message(format!("扫描根目录 (线程数: {})", threads));
        progress_bar.set_style(ProgressStyle::default_bar()
            .template("{msg} [{elapsed_precise}] [{bar:50}] {pos}/{len} ({eta})")
            .unwrap()
            .progress_chars("=>")); // 使用"=>"，这会显示为[==============>    ]
        
        debug!("开始扫描根目录: {} 个URL", root_url_count);
        
        // 1. 先扫描根目录
        let _root_results = self.scan_url_batch(client, root_urls, verify_content, progress_bar.clone(), results.clone(), semaphore.clone()).await;
        
        // 等待一小段时间再继续
        tokio::time::sleep(Duration::from_millis(500)).await;
        
        // 如果有子目录，继续扫描
        if !backup_urls.is_empty() {
            debug!("开始扫描备份目录: {} 个URL", backup_urls.len());
            
            // 重置进度条
            progress_bar.set_position(0);
            progress_bar.set_length(backup_urls.len() as u64);
            progress_bar.set_message(format!("扫描备份目录 (线程数: {})", threads));
            
            // 2. 再扫描备份目录
            let _backup_results = self.scan_url_batch(client, backup_urls, verify_content, progress_bar.clone(), results.clone(), semaphore.clone()).await;
        }
        
        // 打印扫描耗时
        let duration = start_time.elapsed();
        debug!("扫描完成，耗时: {:?}", duration);
        
        // 获取最终结果
        let result_clone = {
            let guard = results.lock().unwrap();
            let cloned = guard.clone();
            
            // 如果没有找到任何结果，显示提示信息
            if cloned.is_empty() {
                println!("未发现任何备份文件");
            } else {
                println!("总共发现 {} 个备份文件", cloned.len());
            }
            
            cloned
        };
        
        result_clone
    }
    
    /// 扫描一批URL
    async fn scan_url_batch(&self, client: &HttpClient, urls: Vec<String>, verify_content: bool, 
                           progress_bar: ProgressBar, results: Arc<Mutex<Vec<ScanResult>>>, semaphore: Arc<Semaphore>) -> bool {
        // 对每个URL进行处理
        let mut tasks = Vec::with_capacity(urls.len());
        let urls_count = urls.len();
        
        for url in urls {
            let semaphore = semaphore.clone();
            let client = client.clone();
            let results = results.clone();
            let progress_bar = progress_bar.clone();
            let self_ref = self.clone();
            
            let task = tokio::spawn(async move {
                let _permit = semaphore.acquire().await.expect("信号量错误");
                
                // 添加整体超时保护 - 使用较小的超时值，确保不会单个请求卡住太久
                let timeout_duration = Duration::from_secs(std::cmp::min(self_ref.config.timeout, 10)); // 最多10秒
                let url_check = tokio::time::timeout(
                    timeout_duration,
                    client.check_url(&url, verify_content)
                ).await;
                
                match url_check {
                    Ok(check_result) => match check_result {
                        Ok(Some(result)) => {
                            // 更新模式成功率
                            self_ref.update_pattern_success_rate(&url, true);
                            
                            // 根据不同状态码提供不同提示
                            let discovery_type = match result.status_code {
                                200 => {
                                    // 获取文件大小的可读格式
                                    let size_info = if let Some(size) = result.content_length {
                                        format!("({} KB)", size / 1024)
                                    } else {
                                        String::from("(未知大小)")
                                    };
                                    
                                    format!("✅ 确认备份文件 [200] {}", size_info)
                                },
                                403 => "🔒 受限备份文件 [403]".to_string(),
                                301 | 302 | 307 | 308 => "🔄 重定向备份文件".to_string(),
                                _ => format!("⚠️ 可能的备份文件 [{}]", result.status_code),
                            };
                            
                            // 确保显示发现的备份文件URL
                            println!("发现: {} - {}", url, discovery_type);
                            
                            // 将结果立即保存到临时JSON文件
                            if let Some(output_file) = &self_ref.config.output_file {
                                let temp_file = format!("{}.part", output_file.display());
                                if let Err(e) = self_ref.save_result_to_json(&result, &temp_file) {
                                    debug!("保存结果到临时文件失败: {:?}", e);
                                } else {
                                    debug!("成功保存发现的备份文件到: {}", temp_file);
                                    // 复制到最终结果文件
                                    let _ = std::fs::copy(&temp_file, output_file);
                                }
                            }
                            
                            let mut results_guard = results.lock().unwrap();
                            results_guard.push(result);
                        },
                        Ok(None) => {
                            // 更新模式失败率
                            self_ref.update_pattern_success_rate(&url, false);
                        },
                        Err(e) => {
                            // 错误也计入失败率
                            self_ref.update_pattern_success_rate(&url, false);
                            debug!("请求错误: {:?}", e);
                        }
                    },
                    Err(_) => {
                        // 整体超时，记录失败
                        self_ref.update_pattern_success_rate(&url, false);
                        debug!("请求超时: {}", url);
                    }
                }
                
                progress_bar.inc(1);
            });
            
            tasks.push(task);
        }
        
        // 等待任务批次完成，设置更合理的超时
        // 为每个URL分配3秒，但不超过2分钟
        let per_url_time_ms = 3000; // 3秒/URL
        let max_timeout_ms = 120_000; // 2分钟
        let batch_timeout_ms = std::cmp::min(urls_count as u64 * per_url_time_ms, max_timeout_ms);
        let batch_timeout = Duration::from_millis(batch_timeout_ms);
        
        match tokio::time::timeout(batch_timeout, future::join_all(tasks)).await {
            Ok(_) => {
                // 正常完成
                progress_bar.finish_with_message("批次扫描完成");
                true
            },
            Err(_) => {
                // 超时，但继续处理部分结果
                progress_bar.finish_with_message("批次扫描部分完成（超时）");
                println!("警告: 批次扫描超时，部分URL未完成检查");
                false
            }
        }
    }
}

impl Clone for Scanner {
    fn clone(&self) -> Self {
        Scanner {
            config: self.config.clone(),
            client: self.client.clone(),
            pattern_success_rates: self.pattern_success_rates.clone(),
            current_threads: self.current_threads.clone(),
        }
    }
} 