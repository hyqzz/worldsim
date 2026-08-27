# WorldSim Demos & Visual Documentation

每个大类一个独立 demo，包含：可视化输出 + 代码讲解 + GIF 动图。

## 前置条件

```bash
# 1. 编译 release 版本
cargo build -p worldsim --release

# 2. 安装 Python SDK
pip install -e sdk/python

# 3. 生成所有 demo
python demos/run_all.py
# 或者单个 demo:
python demos/rigid_bodies.py
```

## Demo 清单

| # | 文件 | 类别 | 演示内容 | 输出 |
|---|---|---|---|---|
| 1 | `rigid_bodies.py` | 刚体 | 自由落体、弹跳、碰撞交换、堆叠稳定 | `output/rigid_bodies.gif` |
| 2 | `fluid_dam_break.py` | 流体 | 水坝溃坝：液体从左向右扩散 | `output/dam_break.gif` |
| 3 | `buoyancy_demo.py` | 耦合 | 木头浮、钢铁沉、铜球悬浮：密度决定命运 | `output/buoyancy.gif` |
| 4 | `heat_diffusion.py` | 热场 | 热点扩散：从一个热源向四周扩散 | `output/heat_diffusion.gif` |
| 5 | `nbody_orbits.py` | N体 | 二体圆轨道 + 三体混沌 | `output/orbits.gif` |
| 6 | `snapshot_fork.py` | 快照/分叉 | 同一世界分叉成两个平行宇宙 | `output/snapshot_fork.png` |
| 7 | `batch_training.py` | 训练 | 64 个并行世界同时运行 | `output/batch_training.png` |
| 8 | `qos_precision.py` | QoS | Low/Medium/High 精度对比 | `output/qos_precision.png` |
| 9 | `wdl_scene.py` | WDL | 声明式场景描述 | `output/wdl_scene.png` |
| 10 | `full_demo.py` | 全场景 | 水箱 + 球 + 热源 + 探针 | `output/full_demo.gif` |

## 自定义 Demo

每个 demo 都是一个独立的 Python 脚本。修改参数即可探索不同场景：

```python
# 修改 fluid_dam_break.py 中的粒子数量
FLUID_H = 0.04    # 更小 = 更多粒子 = 更精细
FLUID_RADIUS = 1.2 # 更大 = 更多水
```
