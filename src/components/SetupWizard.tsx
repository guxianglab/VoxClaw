import { useState } from "react";
import { AiPane, MicrophoneSection, SpeechPane } from "./settings/panes";
import { isAiReady, isSpeechReady, useConfigDraft } from "./settings/useConfigDraft";
import { ActionButton, Key, Notice, PrimaryButton } from "./ui";

const STEPS = [
  { title: "选择麦克风", desc: "VoxClaw 会用它来听你说话" },
  { title: "设置语音识别", desc: "选择把声音变成文字的方式" },
  { title: "连接 AI（可选）", desc: "用于润色文字、语音指令和会议总结，可以以后再配置" },
  { title: "准备好了", desc: "试着在任意输入框里说一句话" },
];

/** First-run guide. Every step auto-saves, so leaving early keeps progress. */
export function SetupWizard({ onDone }: { onDone: () => void }) {
  const { config, update, flush } = useConfigDraft(true);
  const [step, setStep] = useState(0);

  if (!config) return null;

  const finish = async () => {
    await flush();
    onDone();
  };

  const isLast = step === STEPS.length - 1;
  const canNext = step !== 1 || isSpeechReady(config);
  const pane = { config, update };
  const key = config.trigger_mouse ? "鼠标中键" : "右 Alt";

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-neutral-100 p-4">
      <div className="flex h-[min(680px,92vh)] w-full max-w-2xl flex-col bg-neutral-50">
        <div className="border-b border-neutral-200 px-8 pb-5 pt-8">
          <div className="flex gap-1.5">
            {STEPS.map((_, i) => (
              <span key={i} className={`h-1 flex-1 rounded-full ${i <= step ? "bg-chinese-indigo" : "bg-neutral-200"}`} />
            ))}
          </div>
          <div className="mt-5 text-xs text-neutral-400">
            第 {step + 1} 步，共 {STEPS.length} 步
          </div>
          <h2 className="mt-1 text-xl font-semibold text-neutral-900">{STEPS[step].title}</h2>
          <p className="mt-1 text-sm text-neutral-500">{STEPS[step].desc}</p>
        </div>

        <div className="custom-scrollbar min-h-0 flex-1 overflow-y-auto px-8 py-6">
          {step === 0 && <MicrophoneSection {...pane} />}
          {step === 1 && <SpeechPane {...pane} />}
          {step === 2 && <AiPane {...pane} />}
          {step === 3 && (
            <div className="space-y-6 text-sm text-neutral-700">
              <UsageRow keys={[key]} action="单击" result="开始听写，再按一次结束，文字会输入到光标处" />
              <UsageRow keys={[key]} action="双击" result="这一次的文字交给 AI 润色后再输入" />
              <UsageRow keys={[key]} action="长按" result="对 AI 助手说指令，比如“打开 B 站”" />
              {!isAiReady(config) && (
                <Notice>还没有连接 AI，润色和 AI 助手暂不可用。之后可以在「设置 → AI 模型」里补上。</Notice>
              )}
            </div>
          )}
        </div>

        <div className="flex items-center justify-between border-t border-neutral-200 px-8 py-4">
          {isLast ? <span /> : <ActionButton onClick={finish}>稍后设置</ActionButton>}
          <div className="flex items-center gap-2">
            {step > 0 && !isLast && <ActionButton onClick={() => setStep(step - 1)}>上一步</ActionButton>}
            {step === 2 && !isAiReady(config) && <ActionButton onClick={() => setStep(3)}>跳过</ActionButton>}
            {isLast ? (
              <PrimaryButton onClick={finish}>开始使用</PrimaryButton>
            ) : (
              <PrimaryButton onClick={() => setStep(step + 1)} disabled={!canNext}>
                下一步
              </PrimaryButton>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

function UsageRow({ keys, action, result }: { keys: string[]; action: string; result: string }) {
  return (
    <div className="flex items-start gap-4">
      <div className="flex w-36 flex-shrink-0 items-center gap-1.5">
        {keys.map((k) => (
          <Key key={k}>{k}</Key>
        ))}
        <span className="text-neutral-900">{action}</span>
      </div>
      <div className="leading-6 text-neutral-500">{result}</div>
    </div>
  );
}
