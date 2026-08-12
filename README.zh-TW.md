# DualSense Mapper

[English](README.md) | **繁體中文**

把 PS5 DualSense 控制器映射成鍵盤按鍵，讓筆電也能用手把玩。2025 年 5 月最初用 Python 寫成，為了讓我太太在 MacBook 上舒服地玩《新楓之谷 Worlds》Artale；後來用 Rust 重寫，打包成單一個 Windows 執行檔。

<p align="center">
  <img src="docs/images/main-window.png" width="820"
       alt="DualSense Mapper — Mappings 頁籤：可互動的控制器圖，下方是每顆按鈕的按鍵綁定">
  <br>
  <sub>直接點控制器圖上的按鈕就能綁定。按鈕名稱來自你的設定檔，可以改成任何你想要的字。</sub>
</p>

## 這工具給你什麼

- **點圖綁定，不必編輯 JSON。** 點控制器圖上的按鈕（或下方對應的那一列），選 Key / Macro / Mouse / Unbound。
- **29 個可映射輸入** —— 四個面板鍵、D-pad、左右蘑菇頭各四向、L1 / R1、L2 / R2 類比板機、L3 / R3、Share / Options / PS，再加觸控板四個象限。
- **觸控板可當滑鼠游標用**，帶兩段式加速曲線；四個象限各自是獨立的點擊綁定。
- **巨集延遲隨機化。** 每個步驟的延遲都是 `[min, max]` 區間，不存在固定節拍 —— 循環巨集不會被辨識成腳本特徵。
- **不會卡鍵。** 每一次合成按下都經過同一層帶引用計數的 safety 層；`Drop` 與 panic hook 保證即使 process 死掉，仍握著的鍵全部放開。
- **可以直接從 app 關掉控制器**（Windows）—— 送的是 Bluetooth link 層 disconnect，配對保留，按 PS 鍵就能重連，跟 PS5 主機一樣。
- **只有一個約 11 MB 的 `.exe`。** 不用安裝程式、沒有 DLL、沒有驅動、不做 process hooking —— 只用 user-mode `SendInput`。設定是首次執行時寫在 exe 旁邊的單一 JSON 檔。

## 怎麼取得

到 [latest release](https://github.com/Luotee/dualsense-mapper/releases) 下載 `dualsense-mapper.exe`，用藍牙配對手把，然後雙擊執行檔。完整步驟與按鈕對照表見 [`rust/README.md`](rust/README.md)（英文）。

## 兩套實作

| 目錄 | 狀態 | 對象 |
|---|---|---|
| `legacy-python/` | 可用，凍結作為參考 | 習慣 `pip install` 的開發者 |
| `rust/` | Phase 1（Windows），Phase 2（macOS）進行中 | 一般使用者 —— 單一 `.exe` |

推薦走 Rust 版。Python 版保留是為了 blame 歷史，也因為在 Phase 2 落地前它在 macOS 上仍可用。

## 支援的硬體

- **DualSense PS5 控制器**（`054c:0ce6`），走 **Bluetooth**。

目前還不支援：

- DualSense 走 USB（延到 v2.0.1）。
- DualSense Edge（`054c:0df2`，延到 v2.0.1）。
- Xbox / 8BitDo / 一般 XInput 手把 —— v1.2.0 是最後一個帶 gilrs 泛用手把路徑的版本；v2.0.0 改成 DualSense 專用的 raw HID reader，好讓連線狀態、觸控板、IMU 與電量能直接從 78 bytes 的 HID report 讀出來。

## 為什麼有這個專案

現有的映射工具在負載高時會漏掉 key-release 事件，鍵就「卡住」了。Python 原型用三層 release-on-exit 防線解掉這件事，並做了延遲隨機化的巨集引擎，讓帶腳本味的輸入模式不會被線上遊戲標記。Rust 重寫版兩者都保留，另外修掉 Python 版兩個潛在 bug（板機 idle 值跨平台不一致；共用鍵的 release 衝撞），並打包給非技術使用者。

延伸閱讀：

- [`rust/README.md`](rust/README.md) —— 建置、執行、按鈕對照（英文）
- [`legacy-python/README.md`](legacy-python/README.md) —— 原始 Python 版說明（英文）
- [`CHANGELOG.md`](CHANGELOG.md) —— 各版本變更紀錄（英文）
