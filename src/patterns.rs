use crate::Result;
use log::debug;
use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use url::Url;

/// 备份文件模式生成器
pub struct PatternGenerator {
    pub prefixes: Vec<String>,        // 前缀，将与后缀组合
    pub full_paths: Vec<String>,      // 完整路径，不与后缀组合
    pub hard_coded_suffixes: Vec<String>,  // 硬编码的后缀列表
    pub domain_placeholders: Vec<String>,  // 域名占位符模板
    pub backup_dirs: Vec<String>,     // 备份目录名称
}

impl PatternGenerator {
    /// 创建一个新的模式生成器
    pub fn new() -> Self {
        let hard_coded_suffixes = vec![
            ".rar".to_string(),
            ".zip".to_string(),
            ".tar.gz".to_string(),
            ".tar".to_string(),
            ".7z".to_string(),
            ".bak".to_string(),
        ];

        let domain_placeholders = vec![
            "{domain}".to_string(),
            "backup-{domain}".to_string(),
            "{domain}-backup".to_string(),
            "bak-{domain}".to_string(),
            "{domain}-bak".to_string(),
            "www-{domain}".to_string(),
            "{domain}-old".to_string(),
            "old-{domain}".to_string(),
            "{domain}-archive".to_string(),
            "archive-{domain}".to_string(),
            "{domain}_backup".to_string(),
            "backup_{domain}".to_string(),
            "{domain}_bak".to_string(),
            "bak_{domain}".to_string(),
            "{domain}_old".to_string(),
            "old_{domain}".to_string(),
            "{domain}_archive".to_string(),
            "archive_{domain}".to_string(),
        ];
        
        let backup_dirs = vec![
            "backup".to_string(),
            "backups".to_string(),
        ];

        Self {
            prefixes: Vec::new(),
            full_paths: Vec::new(),
            hard_coded_suffixes,
            domain_placeholders,
            backup_dirs,
        }
    }

    /// 从文件加载自定义模式
    pub fn load_custom_patterns<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let mut loaded_prefixes = 0;
        let mut loaded_full_paths = 0;
        
        for line in reader.lines() {
            let line = line?;
            let trimmed = line.trim();
            
            if !trimmed.is_empty() && !trimmed.starts_with('#') {
                // 检查是否以.开头
                if trimmed.starts_with('.') || trimmed.contains('/') {
                    // 以.开头或含有/的作为完整路径，不再组合后缀
                    self.full_paths.push(trimmed.to_string());
                    loaded_full_paths += 1;
                } else {
                    // 不以.开头的作为前缀，将与后缀组合
                    self.prefixes.push(trimmed.to_string());
                    loaded_prefixes += 1;
                }
            }
        }
        
        debug!("从模式文件加载了 {} 个前缀和 {} 个完整路径", loaded_prefixes, loaded_full_paths);
        
        // 如果没有加载任何模式，使用默认的一些值
        if self.prefixes.is_empty() && self.full_paths.is_empty() {
            self.prefixes = vec![
                "backup".to_string(),
                "bak".to_string(),
            ];
            debug!("使用默认前缀模式: {:?}", self.prefixes);
        }
        
        Ok(())
    }

    /// 从文件加载自定义域名占位符模板
    pub fn load_custom_domain_placeholders<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        
        for line in reader.lines() {
            let line = line?;
            let trimmed = line.trim();
            
            if !trimmed.is_empty() && !trimmed.starts_with('#') {
                self.domain_placeholders.push(trimmed.to_string());
            }
        }
        
        Ok(())
    }

    /// 获取常见备份名称，用于初步检测
    pub fn get_common_backup_names(&self) -> Vec<String> {
        vec![
            "backup".to_string(),
            "back".to_string(),
            "bak".to_string(),
        ]
    }

    /// 为给定的URL生成所有可能的备份文件URL
    pub fn generate_urls(&self, target_url: &str) -> Result<Vec<String>> {
        let url = Url::parse(target_url)?;
        let host = url.host_str().ok_or_else(|| {
            crate::BackerError::Config(format!("无效的URL: {}", target_url))
        })?;
        
        let domain = extract_domain(host);
        debug!("从 {} 提取的域名部分: {}", host, domain);
        
        let base_url = format!("{}://{}", url.scheme(), host);
        
        // 先生成根目录URL
        let mut root_urls: HashSet<String> = HashSet::new();
        self.generate_root_urls(&mut root_urls, &base_url, &domain);
        
        // 再生成子目录URL
        let mut dir_urls: HashSet<String> = HashSet::new();
        self.generate_backup_dir_urls(&mut dir_urls, &base_url, &domain);
        
        // 统计根目录URL数量
        let root_urls_count = root_urls.len();
        
        // 将根目录URL放在前面
        let mut result_vec = root_urls.into_iter().collect::<Vec<String>>();
        result_vec.extend(dir_urls.into_iter());
        
        debug!("为目标 {} 生成了 {} 个备份文件URL (根目录: {})", 
               target_url, result_vec.len(), root_urls_count);
        
        Ok(result_vec)
    }
    
    /// 为根目录生成备份文件URL
    fn generate_root_urls(&self, result: &mut HashSet<String>, base_url: &str, domain: &str) {
        // 1. 添加完整路径（不添加后缀）
        for path in &self.full_paths {
            result.insert(format!("{}/{}", base_url, path));
        }
        
        // 2. 前缀与硬编码后缀组合
        for prefix in &self.prefixes {
            // 检查前缀是否已经包含后缀（如 "backup.zip"）
            if prefix.contains('.') {
                // 如果已包含后缀，直接添加
                result.insert(format!("{}/{}", base_url, prefix));
            } else {
                // 否则组合所有后缀
                for suffix in &self.hard_coded_suffixes {
                    result.insert(format!("{}/{}{}", base_url, prefix, suffix));
                }
            }
        }
        
        // 3. 域名本身与硬编码后缀组合
        for suffix in &self.hard_coded_suffixes {
            result.insert(format!("{}/{}{}", base_url, domain, suffix));
        }
        
        // 4. 域名变体与硬编码后缀组合
        let domain_variants = self.generate_domain_variants(domain);
        for variant in domain_variants {
            for suffix in &self.hard_coded_suffixes {
                result.insert(format!("{}/{}{}", base_url, variant, suffix));
            }
        }
    }
    
    /// 为备份目录生成备份文件URL
    fn generate_backup_dir_urls(&self, result: &mut HashSet<String>, base_url: &str, domain: &str) {
        for dir in &self.backup_dirs {
            // 1. 目录下的域名与后缀组合
            for suffix in &self.hard_coded_suffixes {
                result.insert(format!("{}/{}/{}{}", base_url, dir, domain, suffix));
            }
            
            // 2. 目录下的通用备份名
            for common_name in &["backup", "site", "www", "web", "database", "db"] {
                for suffix in &self.hard_coded_suffixes {
                    result.insert(format!("{}/{}/{}{}", base_url, dir, common_name, suffix));
                }
            }
            
            // 3. 目录下前缀与后缀组合
            for prefix in &self.prefixes {
                // 检查前缀是否已经包含后缀
                if prefix.contains('.') {
                    // 如果已包含后缀，直接添加
                    result.insert(format!("{}/{}/{}", base_url, dir, prefix));
                } else {
                    // 否则组合所有后缀
                    for suffix in &self.hard_coded_suffixes {
                        result.insert(format!("{}/{}/{}{}", base_url, dir, prefix, suffix));
                    }
                }
            }
            
            // 4. 目录下完整路径
            for path in &self.full_paths {
                if path.starts_with('.') {
                    // 对于.开头的路径，添加不带前导点的版本
                    let no_dot = path.trim_start_matches('.');
                    if !no_dot.is_empty() {
                        result.insert(format!("{}/{}/{}", base_url, dir, no_dot));
                    }
                }
                // 始终添加原始路径
                result.insert(format!("{}/{}/{}", base_url, dir, path));
            }
            
            // 5. 目录下的域名变体与后缀组合
            let domain_variants = self.generate_domain_variants(domain);
            for variant in domain_variants {
                for suffix in &self.hard_coded_suffixes {
                    result.insert(format!("{}/{}/{}{}", base_url, dir, variant, suffix));
                }
            }
        }
    }
    
    /// 生成域名的各种变体
    fn generate_domain_variants(&self, domain: &str) -> Vec<String> {
        let mut variants = Vec::new();
        
        // 基本变体
        variants.push(domain.to_string());
        
        // 使用占位符模板替换域名
        for placeholder in &self.domain_placeholders {
            let replaced = placeholder.replace("{domain}", domain);
            variants.push(replaced);
        }
        
        // 移除非字母数字字符，创建纯净版本
        let clean_domain: String = domain.chars()
            .filter(|c| c.is_alphanumeric())
            .collect();
        if clean_domain != domain {
            variants.push(clean_domain);
        }
        
        variants
    }
}

/// 从主机名提取域名部分
fn extract_domain(host: &str) -> String {
    // 如果是IP地址，直接返回
    if host.chars().all(|c| c.is_digit(10) || c == '.') {
        return host.to_string();
    }

    // 尝试提取二级域名
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() >= 2 {
        // 如果是常见的二级域名，如 .co.uk, .com.au 等
        if parts.len() > 2 && parts[parts.len() - 2].len() <= 3 {
            if parts.len() > 3 {
                return parts[parts.len() - 3].to_string();
            }
        } else {
            return parts[parts.len() - 2].to_string();
        }
    }
    
    host.to_string()
}

impl Clone for PatternGenerator {
    fn clone(&self) -> Self {
        Self {
            prefixes: self.prefixes.clone(),
            full_paths: self.full_paths.clone(),
            hard_coded_suffixes: self.hard_coded_suffixes.clone(),
            domain_placeholders: self.domain_placeholders.clone(),
            backup_dirs: self.backup_dirs.clone(),
        }
    }
} 