# WE 效果栈设计（二期）

> 一句话：把 Wallpaper Engine scene 里的 shader 效果（水波、火焰摆动、树叶
> 摇摆、画面抖动等）在 Gesso 的 webview 里实时跑起来，让导入的场景壁纸
> 「动起来」。一期已完成静态图层渲染（效果链跳过并如实告知）；本文档是
> 效果栈与 spritesheet 搭车项的设计与实施依据。
>
> **事实来源 = 本机真实工坊文件**（用户导入并解包）：
> - PixelFuture（工坊 2838823495）：4 类效果（waterripple / foliagesway /
>   fire / shake）、5 个效果实例、16 组粒子（粒子不在本设计范围）；
> - Cyberpunk city 16BIT（工坊 2228701016）：spritesheet 帧序列（100 帧）。
> 文中所有格式断言均可在这两个条目的文件里找到出处。

## 1. 目标与验收

- PixelFuture：水波扭曲与火焰着色可见，树叶摆动与抖动生效（shader 顶点动画）；
- Cyberpunk：LED 帧序列按元数据播放（不再显示整张拼合图）；
- 不支持的构件（粒子等）维持跳过 + toast 如实告知，不回归；
- 现有静态路径零回归（Cyberpunk 静态帧与全部内置壁纸照常）。

## 2. 真实格式分析

### 2.1 效果链的声明（scene.json）

`objects[].effects[]` 是**实例列表，数组顺序 = 渲染顺序**。PixelFuture 的
底图层挂了 5 个实例（waterripple、foliagesway×2、fire、shake）。每个实例：

```json
{
  "file": "effects/waterripple/effect.json",
  "id": 68,
  "passes": [{
    "combos": { "PERSPECTIVE": 1 },            // 编译期开关（覆盖注释默认值）
    "constantshadervalues": {                   // uniform 实例值
      "animationspeed": 0.08, "ripplestrength": 0.1, "scale": 1.0,
      "point0": "0 0", "point1": "1 0", "point2": "1 1", "point3": "0 1"
    },
    "textures": [null, "masks/waterripple_mask_1d5ac55e", "effects/waterripplenormal"]
  }],
  "visible": true
}
```

- `combos`：编译期 `#if` 开关的实例覆盖（未覆盖的取 shader 注释里的 default）；
- `constantshadervalues`：键 = shader 注释元数据里的 `"material"` 名；值可为
  数字或空格分隔字符串（向量），解析复用 sceneVec 规则；
- `textures`：贴图槽覆盖，**null = 该槽使用默认语义**（见 3.4）；裸名引用
  复用一期贴图候选链（材质目录优先，.tex 回落 .png/.jpg/.gif）。

### 2.2 effect.json → 材质 pass

effect.json 的 `passes[].material` 指向一个**与图层材质同构**的材质 json
（`materials/effects/waterripple.json`），其 pass 用 `shader` 字段引用自定义
着色器：`"shader": "effects/waterripple"` → `shaders/effects/waterripple.vert`
+ `.frag`。`dependencies` 数组列出该效果的全部依赖（材质 + 贴图 + shader），
可用于实现期的完整性校验。

### 2.3 shader 方言特征（全部 8 个文件扫描结论）

| 特征 | 证据 | 处置 |
|---|---|---|
| GLSL ES 1.00 形态：`attribute` / `varying` / `gl_FragColor` | 全部文件 | 目标语言就用 ES 1.00（WebGL2 接受 ES 1.00 着色器），不做 in/out 升级 |
| `#include "common.h"` / `common_blending.h` / `common_perspective.h` | 全部文件 | WE 运行时库，**条目内不含**——内嵌等价实现（见 3.3） |
| 库函数：`mul`×6、`texSample2D`×15、`rotateVec2`×5、`frac`×5、`saturate`×3、`inverse`×1、`squareToQuad`×1 | 全部文件 | shim 实现（见 3.3） |
| `// [COMBO] {...}` / `[COMBO_OFF]` 注释元数据 | vert/frag | combo 声明源：`{"combo":"MASK","type":"options","default":0}` |
| `#if PERSPECTIVE == 0` / `#if MASK == 1` / `#if SPECULAR == 1` | 全部文件 | 静态分支求值（见 3.2 规则 2） |
| uniform 注释元数据：`{"material":"ripplestrength","default":0.1,"range":[0,1]}`、`{"hidden":true}`、`{"mode":"opacitymask","combo":"MASK"}` | 全部文件 | uniform 供给 + 贴图槽语义 |
| `g_AudioSpectrum16Left/Right[16]`（fire.frag） | fire | **按零数组供给**（音频响应三期），退化如实记录 |

特殊 uniform 供给（WE 运行时职责）：`g_Time`（秒）、
`g_ModelViewProjectionMatrix`（正交 + 层变换，现有 MVP 可复用）、
`g_TextureNResolution` = vec4(1/w, 1/h, w, h)——由 waterripple.vert 中
`g_Texture0Resolution.x / g_Texture0Resolution.y` 的宽高比用法验证。

## 3. 架构

### 3.1 渲染管线：FBO ping-pong pass 链

现状渲染循环没有 FBO 基础设施（一期直绘）。改造：

- **带效果图层**：输入纹理 → pass1（写 RT_A）→ pass2（读 RT_A 写 RT_B）→
  … → 合成阶段用**最终 RT** 当作该层的 `tex`。两块 FBO 交替复用（效果串行，
  ping-pong 足够）；
- RT 尺寸 = 层贴图尺寸（经一期缩放预算后的实际尺寸），RGBA8 + 深度关；
  4K 层两块 RT 约百余 MB 显存，桌面可接受；
- **无效果图层走现有直绘路径，完全不变**（零回归原则）；
- 每效果每帧一次 pass draw：PixelFuture 5 个实例 = 每帧 5 次额外 draw +
  RT 读写，4K 下带宽可控；fps_cap 兜底照旧；
- 合成阶段（现有 draw loop）只改一处：层的纹理来源 = 效果链最终 RT 或原
  纹理。层变换/混合/对齐逻辑复用。

### 3.2 翻译器（宿主页 JS 源变换，规则表）

| # | 规则 | 说明 |
|---|---|---|
| 1 | `#include "common*.h"` → 内嵌 shim 源码 | 三个头各自平移所需子集（3.3） |
| 2 | COMBO 静态求值 | 收集注释声明的 combo（default）→ 实例 `combos` 覆盖 → 对 `#if/#elif/#else/#endif` 块求值；表达式只见过 `==` 与裸名，按白名单求值；未知 combo = 0；**表达式无法按白名单求值 → 整个效果实例跳过**（误选分支会静默出错画面，宁可跳过并计数） |
| 3 | `texSample2D` → `texture2D` | `#define texSample2D texture2D` 即可 |
| 4 | `mul` / `saturate` / `frac` / `rotateVec2` / `squareToQuad` / `inverse` | shim 函数（ES 1.00 支持重载；`inverse(mat3)` 需自写 3×3 求逆）。注意 `mul(v, M)` 是行向量约定，shim 按 v·M 实现，MVP 的行列布局以 catsout 参考实现为准并在实现期与真实效果逐像素比对核实 |
| 5 | 注入 `precision highp float;` | ES 1.00 fragment 必需；桌面 GPU 全支持 highp——水波/透视分支的大数值 UV 计算在 mediump 下有精度风险 |
| 6 | uniform 值运行时供给 | 非源变换，见 3.4 |

- 翻译产物按 `(shader路径, combos序列化)` 缓存——PixelFuture 的 5 个实例
  各编译一次；
- 编译/链接失败 → **该效果实例跳过**（不崩场景），omission 计数
  （键 `effects-unsupported`）。

### 3.3 运行时库 shim（内嵌实现范围）

以真实文件的函数调用清单为准：`mul`（vec×mat 各重载）、`texSample2D`、
`rotateVec2(v, rad)`、`saturate`、`frac`、`squareToQuad(p0..p3)`（三点/四点
二次贝塞尔平方转四边形矩阵）、`inverse(mat3)`。shim 以 GLSL 源码内嵌在宿主
页，替换 include 时注入；实现以「让首批 4 个效果正确编译与运行」为准，
不追求复刻 WE 全库。

### 3.4 uniform 与贴图槽供给

- `g_Time`：现有帧循环 tSec；`g_ModelViewProjectionMatrix`：现有层 MVP；
- `g_TextureNResolution`：按实际绑定的贴图尺寸生成（vec4(1/w, 1/h, w, h)）；
- `constantshadervalues`：按 shader 注释 `"material"` 名匹配注入；声明了
  而实例未提供的 → 取注释 `default`；均无 → 0；
- 贴图槽：实例 `textures[i]` 覆盖优先；`null` 槽语义——slot0 = 当前输入
  （链上一 pass 输出或层贴图）、其余槽 = 效果自带贴图（裸名候选链解析）；
  **联动规则**：注释带 `"mode":"opacitymask"` + `combo` 的槽，该 combo 取值
  由槽是否提供贴图决定（有 → 1，无 → 0）——PixelFuture 的水波实例提供
  mask 贴图，联动做错遮罩会整体失效。

## 4. 搭车项：动图贴图播放

实测定稿（2026-11-15 第三版，自测环境逐项验证）：真实样本 Cyberpunk 的动
图纹理经 repkg 转换输出为 **GIF 动图**（文件头 GIF89a，960×640，扩展名仍
为引用名 `.png`）；`spritesheetsequences` 的 `width/height` 是整图尺寸，
`frames: 100` 是**时间帧序列**（每帧时长 = duration/frames = 10ms）。

- 触发：材质 `combos.spritesheet = 1` 且 tex-json `frames > 1`；
- 实现（实测第四版）：**纯 JS GIF 解码器**（LZW + GCT/LCT 调色板 + 透明
  索引 + interlace 还原 + disposal 0-2 合成，约 200 行宿主页内嵌）——壁纸
  页面 origin（http://gesso.steam workaround）非安全上下文，WebCodecs
  ImageDecoder 不可用（实测 WebView2 157 中 undefined）；播放节拍 = GIF
  自带帧延时（repkg 从 tex 帧信息写入；tex-json 的 duration 语义存疑，
  按整段秒数解读实机过快被否）；帧合成按序进行（GIF 帧间有增量依赖不能
  跳帧），每渲染帧把合成画布重传到 GPU；
- 实测教训（自测环境逐项抓出）：① DOM 动画时钟不适用于游离/不可见图片
  （drawImage 恒为第 0 帧）；② VideoFrame.duration 与 GIF GCE 延时实测
  不一致——节拍改以 GIF 自带帧延时为准（tex-json duration 语义存疑，
  实机校准）；③ 解码失败帧的
  null 占位必须被缓存驱逐跳过（否则每帧 TypeError 杀死渲染循环 = 灰屏）；
  ④ **WebCodecs 仅存在于安全上下文**——壁纸 origin（http://gesso.steam）
  非安全上下文，ImageDecoder undefined（自测环境 127.0.0.1 有 localhost
  豁免，曾掩盖该差异）；⑤ GIF 子块带长度前缀不连续，LZW 输入须先拼接；
- Cyberpunk：100 帧 / 80ms 帧延时 = 8s 循环（12.5fps，环境型慢速演变）；
- **边界**：帧信息仅 repkg 解包产物携带；散装条目维持静态首帧；真正空间
  网格式 spritesheet（如存在）属阶段 C 评估；音频响应（fire 的
  g_AudioSpectrum）按零供给。

## 5. 支持矩阵与退化

| 构件 | 处置 |
|---|---|
| 首批 4 效果（waterripple / foliagesway / fire / shake） | 实现（PixelFuture 全部 5 实例为验收金标准） |
| 未知效果文件 / 未知 combo 结构 / 无法求值的 #if 表达式 | 跳过该效果实例 + omission 计数（`effects-unsupported`） |
| 效果的多 pass（effect.json passes 数组） | 逐 pass 串行渲染，数量不限（管线天然支持；首批真实样本均为单 pass） |
| `g_AudioSpectrum*` | 零数组供给（音频响应三期），差异如实记录 |
| 粒子 / 文本 / 音频 / 3D / puppet | 维持明确不做（现状） |
| 效果 shader 编译失败 | 该实例跳过 + 计数，场景其余部分照常 |

## 6. 分阶段实施

- **阶段 A（动图贴图播放）**：GIF 逐帧重传——验收 = Cyberpunk
  LED 动画播放、PixelFuture 静态底图无回归。不含 FBO（无消费者代码不入库）；
- **阶段 B（FBO 管线 + 翻译器 + 首批效果）**：已实施（2026-11-15）——
  FBO ping-pong pass 链、shim 库、COMBO 求值（含遮罩联动）、uniform 供给、
  4 效果 5 实例全部编译运行。实机校准补记：GLSL ES 1.00 严格类型（裸整数
  补 .0）、combo 标识符需替换进代码体、翻译失败实例跳过不崩场景；效果可见
  强度由条目参数决定（fire/shake 的 noflow 中性流向图 = 零贡献，WE 同），
  遮挡时 WebView2 挂起渲染为省电设计；
- **阶段 C（泛化）**：按真实条目逐个扩效果、组合兜底策略。
- 每阶段独立提交、独立验收；PixelFuture / Cyberpunk 为常备回归金标准。

## 7. 测试与验收

- 翻译器规则表逐条单测（合成 shader 样本覆盖 include 替换、COMBO 求值、
  函数 shim、precision 注入）；**不把工坊 shader 文件入库**（内容不再分发
  边界），真实文件仅实机验收；
- 快扫/导入链回归：现有全部测试不破；效果支持矩阵变化同步 omission 键；
- 实机验收 = 本机两个真实条目（水波可见、LED 动画播放）+ 内置样例零回归。

## 8. 风险与边界

- **兼容性天花板**：WE 官方持续出新效果，翻译器是永久维护尾巴；每未支持
  效果 = 该场景视觉降级，靠导入时 omission 上报兜底（WALLPAPER-ENGINE.md
  已记为设计属性）；
- **性能**：效果 pass 每帧重绘（水波是时间动画，不可缓存首帧）；重场景
  （多层 × 长链 × 4K RT）在弱机可能不达标——fps_cap 与空闲降帧兜底，
  逃生舱（烘焙导出）维持 WALLPAPER-ENGINE.md 的既定设计；
- **语义差异如实记录**：audio 零供给、copiesBackground 语义近似（slot0 读
  输入链而非全屏复制）——差异在 omission / 文档中可见；
- **安全模型不变**：shader 源来自工坊文件 = 不可信输入，翻译后必须过编译
  白名单式 shim 与 COMBO 求值，禁止 eval/动态拼接源码以外的执行路径。

## 9. 粒子运行时（阶段 C 首批，2026-11-15 实施）

「粒子不做」的早期决策基于格式未知；真实条目格式扫描（16 个定义文件，
每个 1.7–4KB）证实可 tractable 实现，纳入支持：

- 原语全覆盖（全量扫描封闭集合）：发射器 boxrandom/sphererandom；
  初始化器 life/size/color/velocity/rotation/angularvelocity/alpha/
  turbulentvelocity random；操作器 movement（gravity/drag）、
  controlpointattract（吸引点绕行，鸟群核心行为）、alphafade、colorchange、
  sizechange、angularmovement、oscillatealpha/position、turbulence；
  渲染器 sprite；
- 实现：CPU 模拟（每系统 ≤maxcount 粒，逐帧发射/更新/回收）+ 复用合成
  管线逐粒子 sprite 绘制（合成着色器加 uTexScale/uTexShift UV 窗口供
  精灵图逐帧采样，常规层恒等零回归）；精灵图帧信息沿用 repkg tex-json；
- rope/spritetrail/ropetrail 拖尾渲染器与未知原语 → 该粒子系统跳过并
  console 记录（后续按需扩）；快扫不再把 particles 列为不支持项；
- 验收 = PixelFuture 16 组鸟群（birds.json：30 粒、球形出生、寿命 25s、
  双吸引点绕行、16 帧扇翅精灵图 sequencemultiplier 30）。
  实测边界（2026-11-15）：repkg 把 birds 的动图纹理转成 GIF 时丢失全部
  alpha 与图案（16 帧全为纯白不透明）——该粒子贴图从转换件无法渲染，运行时
  逐帧采样检测全白帧并跳过该系统（不再渲染白方块）；预设类粒子贴图
  （particle/smoke、fog 等）属于 WE 本体安装资源、不在工坊包内，转换件同样
  缺失 → 跳过。结论：工坊包自带贴图的 sprite 粒子可渲染，依赖 WE 本体资源
  的预设粒子需要用户自行解包 WE 安装目录或后续提供垫片。
