#!/bin/bash

# 编译项目
echo "正在编译 Backer..."
cargo build --release

# 检查编译是否成功
if [ $? -ne 0 ]; then
  echo "编译失败，请检查错误信息"
  exit 1
fi

echo "编译成功！"

# 创建输出目录
mkdir -p results

# 运行扫描工具 - 基本扫描
echo "开始基本扫描..."
./target/release/backer -t targets.txt -p patterns.txt -o results/scan_results.json -f json -j 15

# 运行带有随机请求头和随机IP的扫描
echo "开始高级扫描（随机请求头和随机IP）..."
./target/release/backer -t targets.txt -p patterns.txt -o results/advanced_scan.json -f json -v -j 15 --random-headers --random-ip

# 运行隐蔽模式扫描（禁用随机IP但保留随机请求头）
echo "开始隐蔽模式扫描..."
./target/release/backer -t targets.txt -p patterns.txt -o results/stealth_scan.json -f json -v -j 10 --no-random-ip -T 60

echo "所有扫描完成！结果已保存到 results/ 目录" 