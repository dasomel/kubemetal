import React, { useState, useRef, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import {
  Send,
  Trash2,
  Sliders,
  Sparkles,
  Database,
  Loader2,
  Square,
  Bot,
  User,
  ChevronDown,
  ChevronUp,
  FileText,
  Zap,
  Copy,
  Check,
  ImagePlus,
} from 'lucide-react';
import type { RagSearchResult, ServingRuntime } from '../../types/ipc';

/**
 * 서빙 품질 계측. 스트리밍 응답에서만 채워진다.
 *
 * `exactTokens`가 왜 필요한가: 토큰 수의 정확한 출처는 서버가 보내는 `usage.completion_tokens`
 * 뿐이다. mlx_lm.server가 스트리밍에서 usage를 안 보내는 경우가 있어, 그때는 content delta
 * 청크 수로 근사한다. 근사값을 tok/s라고 단정해 표시하면 안 되므로 플래그로 구분한다.
 */
interface MessagePerf {
  /** 요청 전송부터 첫 content delta까지. 체감 반응성을 가장 잘 나타내는 값이다. */
  ttftMs: number;
  /** 첫 토큰 이후 생성 구간의 처리량 — TTFT를 포함하면 짧은 응답에서 값이 왜곡된다. */
  tokensPerSec: number;
  tokens: number;
  exactTokens: boolean;
  /**
   * tok/s가 서버 보고값인가. mlx-vlm 서버는 매 청크 `timings.predicted_per_second`를
   * 보낸다(실측 0.6.7) — 서버가 직접 잰 값이라 클라이언트 근사보다 정확하므로 우선한다.
   */
  serverTps: boolean;
}

interface Message {
  id: string;
  role: 'system' | 'user' | 'assistant';
  content: string;
  timestamp: number;
  ragSources?: RagSearchResult[];
  perf?: MessagePerf;
  /** 사용자 메시지에 첨부된 이미지(data URL). 표시용이자 API 전송 원본이다. */
  images?: string[];
}

/** OpenAI 호환 멀티모달 콘텐츠. 서버 실측(mlx-vlm 0.6.7)으로 확인한 형태 그대로다. */
type ApiContent =
  | string
  | Array<
      | { type: 'image_url'; image_url: { url: string } }
      | { type: 'text'; text: string }
    >;

interface ModelChatPlaygroundProps {
  port: number;
  modelPath?: string;
  adapterPath?: string;
  /** 'mlx-vlm'일 때만 이미지 첨부가 열린다 — mlx-lm 서버는 image_url을 이해하지 못한다. */
  runtime?: ServingRuntime;
}

const DEFAULT_SYSTEM_PROMPT =
  'You are a helpful, accurate, and concise AI assistant running locally on macOS Apple Silicon via MLX.';

// 프롬프트 상한: 컨텍스트 폭주를 막기 위한 히스토리/RAG 절단 기준.
const MAX_HISTORY_MESSAGES = 8;
const RAG_MAX_CHUNKS = 3;
const RAG_MAX_CHARS = 2000;
const RAG_TRUNCATION_MARK = '… [문자 수 상한으로 절단됨]';

const PRESET_PROMPTS = [
  {
    title: 'KubeMetal 요약',
    prompt: 'KubeMetal의 macOS Host MLX와 Colima K3s 하이브리드 MLOps 아키텍처에 대해 핵심을 요약해줘.',
  },
  {
    title: 'MLX LoRA 예시',
    prompt: 'MLX Python API를 사용하여 로컬 파인튜닝 어댑터를 로드하고 추론하는 예시 코드를 보여줘.',
  },
  {
    title: 'RAG 지식 검색',
    prompt: 'LanceDB 임베디드 벡터 DB와 KubeMetal 호스트의 RAG 연동 구조를 설명해줘.',
  },
  {
    title: 'JSON 응답 테스트',
    prompt: 'KubeMetal의 주요 모듈 3개를 JSON 배열 [ { "name": "...", "description": "..." } ] 포맷으로 출력해줘.',
  },
];

export const ModelChatPlayground: React.FC<ModelChatPlaygroundProps> = ({
  port,
  modelPath,
  adapterPath,
  runtime,
}) => {
  const [messages, setMessages] = useState<Message[]>([]);
  const [input, setInput] = useState('');
  const [isStreaming, setIsStreaming] = useState(true);
  const [temperature, setTemperature] = useState(0.7);
  const [maxTokens, setMaxTokens] = useState(512);
  const [systemPrompt, setSystemPrompt] = useState(DEFAULT_SYSTEM_PROMPT);
  const [showSettings, setShowSettings] = useState(false);

  // RAG state
  const [ragEnabled, setRagEnabled] = useState(true);
  const [ragTopK, setRagTopK] = useState(3);
  const [ragSearching, setRagSearching] = useState(false);

  // Generation state
  const [loading, setLoading] = useState(false);
  const [copiedId, setCopiedId] = useState<string | null>(null);
  // 첨부 대기 이미지(data URL). VLM 런타임에서만 채워진다.
  const [pendingImages, setPendingImages] = useState<string[]>([]);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const vlmActive = runtime === 'mlx-vlm';
  const [expandedSources, setExpandedSources] = useState<Record<string, boolean>>({});

  const messagesEndRef = useRef<HTMLDivElement>(null);
  const abortControllerRef = useRef<AbortController | null>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);

  const scrollToBottom = () => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  };

  useEffect(() => {
    scrollToBottom();
  }, [messages, loading]);

  // 언마운트 시 진행 중인 스트림/요청을 중단해 백그라운드에서 상태 업데이트가 발생하지 않도록 한다.
  useEffect(() => {
    return () => {
      abortControllerRef.current?.abort();
    };
  }, []);

  const handleCopy = (id: string, text: string) => {
    navigator.clipboard.writeText(text);
    setCopiedId(id);
    setTimeout(() => setCopiedId(null), 2000);
  };

  const toggleSourceExpand = (id: string) => {
    setExpandedSources((prev) => ({ ...prev, [id]: !prev[id] }));
  };

  const MAX_ATTACHED_IMAGES = 3;

  /** 파일 선택·클립보드 붙여넣기·드래그앤드롭이 전부 이 하나의 수집기를 지난다 —
   *  이미지 타입 필터와 첨부 상한을 한 곳에서만 지키기 위해서다. */
  const addImageFiles = (files: File[]) => {
    if (!vlmActive) return;
    for (const file of files) {
      if (!file.type.startsWith('image/')) continue;
      const reader = new FileReader();
      reader.onload = () => {
        const url = reader.result;
        if (typeof url === 'string') {
          setPendingImages((prev) =>
            prev.length >= MAX_ATTACHED_IMAGES ? prev : [...prev, url],
          );
        }
      };
      reader.readAsDataURL(file);
    }
  };

  const handlePickImages = (e: React.ChangeEvent<HTMLInputElement>) => {
    const files = Array.from(e.target.files ?? []);
    e.target.value = ''; // 같은 파일 재선택도 change로 잡히게 리셋
    addImageFiles(files);
  };

  /** 클립보드의 이미지(스크린샷 등)를 붙여넣는다. 텍스트 붙여넣기는 건드리지 않는다 —
   *  items에 이미지가 있을 때만 소비하고, 그 경우에도 기본 동작을 막지 않으면
   *  파일명이 텍스트로 함께 들어오는 브라우저가 있어 preventDefault 한다. */
  const handlePaste = (e: React.ClipboardEvent) => {
    if (!vlmActive) return;
    const imageFiles = Array.from(e.clipboardData.items)
      .filter((it) => it.kind === 'file' && it.type.startsWith('image/'))
      .map((it) => it.getAsFile())
      .filter((f): f is File => f !== null);
    if (imageFiles.length > 0) {
      e.preventDefault();
      addImageFiles(imageFiles);
    }
  };

  /** HTML5 드롭. tauri.conf.json에서 dragDropEnabled:false를 명시해야 동작한다 —
   *  Tauri v2 기본값(true)은 네이티브 핸들러가 드롭을 가로채 JS drop 이벤트가 아예
   *  발생하지 않는다. */
  const handleDrop = (e: React.DragEvent) => {
    if (!vlmActive) return;
    e.preventDefault();
    addImageFiles(Array.from(e.dataTransfer.files));
  };

  const handleClearHistory = () => {
    if (loading) {
      abortControllerRef.current?.abort();
    }
    setMessages([]);
  };

  const handleSend = async (userPromptText?: string) => {
    const textToSend = userPromptText || input;
    if (!textToSend.trim() || loading) return;
    // 전송 시점 스냅샷 — 스트리밍 도중 새 첨부가 이번 요청에 섞이지 않게 한다.
    const imagesToSend = vlmActive ? [...pendingImages] : [];

    const userMsgId = `user-${Date.now()}`;
    const assistantMsgId = `assistant-${Date.now()}`;

    let retrievedSources: RagSearchResult[] = [];
    let augmentedPrompt = textToSend.trim();

    // 1. RAG context injection if enabled
    if (ragEnabled) {
      setRagSearching(true);
      try {
        const results = await invoke<RagSearchResult[]>('query_rag', {
          query: textToSend.trim(),
          topK: ragTopK,
        });
        if (results && results.length > 0) {
          // 상위 3청크만 사용하고, 합산 2,000자 상한을 넘는 부분은 절단 표기와 함께 잘라낸다.
          const limitedResults = results.slice(0, RAG_MAX_CHUNKS);
          let remainingChars = RAG_MAX_CHARS;
          const contextBlocks = limitedResults
            .map((r, idx) => {
              const header = `[문맥 ${idx + 1} (출처: ${r.filename || r.source || '문서'}, 유사도 거리: ${r.score.toFixed(3)})]\n`;
              const budget = remainingChars - header.length;
              if (budget <= 0) return null;
              const text =
                r.text.length > budget ? `${r.text.slice(0, budget)}${RAG_TRUNCATION_MARK}` : r.text;
              remainingChars -= header.length + text.length;
              return `${header}${text}`;
            })
            .filter((block): block is string => block !== null);

          retrievedSources = limitedResults.slice(0, contextBlocks.length);

          if (contextBlocks.length > 0) {
            augmentedPrompt = `[LanceDB RAG 참조 문맥]:\n${contextBlocks.join('\n\n')}\n\n[사용자 질의]:\n${textToSend.trim()}`;
          }
        }
      } catch (err) {
        console.warn('RAG Context Injection 건너뜀 (질의 실패 또는 미설치):', err);
      } finally {
        setRagSearching(false);
      }
    }

    const newUserMsg: Message = {
      id: userMsgId,
      role: 'user',
      content: textToSend.trim(),
      timestamp: Date.now(),
      ragSources: retrievedSources.length > 0 ? retrievedSources : undefined,
      images: imagesToSend.length > 0 ? imagesToSend : undefined,
    };

    const newAssistantMsg: Message = {
      id: assistantMsgId,
      role: 'assistant',
      content: '',
      timestamp: Date.now(),
    };

    setMessages((prev) => [...prev, newUserMsg, newAssistantMsg]);
    setInput('');
    setPendingImages([]);
    setLoading(true);

    const abortController = new AbortController();
    abortControllerRef.current = abortController;

    // Prepare API history — 최근 8턴(메시지)만 유지해 프롬프트 폭주를 방지한다.
    // 과거 턴의 이미지는 다시 보내지 않는다(같은 상한 원칙): 이미지 토큰이 텍스트보다
    // 수십 배 크고, 실측상 후속 질문은 직전 응답 텍스트로 충분히 이어진다.
    const userContent: ApiContent =
      imagesToSend.length > 0
        ? [
            ...imagesToSend.map((url) => ({ type: 'image_url' as const, image_url: { url } })),
            { type: 'text' as const, text: augmentedPrompt },
          ]
        : augmentedPrompt;
    const apiMessages: Array<{ role: string; content: ApiContent }> = [
      { role: 'system', content: systemPrompt },
      ...messages
        .filter((m) => m.role === 'user' || m.role === 'assistant')
        .slice(-MAX_HISTORY_MESSAGES)
        .map((m) => ({ role: m.role, content: m.content as ApiContent })),
      { role: 'user', content: userContent },
    ];

    const endpoint = `http://127.0.0.1:${port}/v1/chat/completions`;

    try {
      if (isStreaming) {
        const t0 = performance.now();
        let tFirstToken: number | null = null;
        let deltaChunks = 0;
        let usageTokens: number | null = null;
        let serverTps: number | null = null;

        const response = await fetch(endpoint, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            model: modelPath || 'default',
            messages: apiMessages,
            temperature,
            max_tokens: maxTokens,
            stream: true,
          }),
          signal: abortController.signal,
        });

        if (!response.ok) {
          const errText = await response.text();
          throw new Error(`서버 응답 오류 (${response.status}): ${errText}`);
        }

        if (!response.body) {
          throw new Error('응답 스트림을 생성할 수 없습니다.');
        }

        const reader = response.body.getReader();
        const decoder = new TextDecoder('utf-8');
        let accumulated = '';
        let buffer = '';

        while (true) {
          const { done, value } = await reader.read();
          if (done) break;

          buffer += decoder.decode(value, { stream: true });
          const lines = buffer.split('\n');
          buffer = lines.pop() || '';

          for (const line of lines) {
            const trimmed = line.trim();
            if (!trimmed || trimmed.startsWith(':')) continue;
            if (trimmed === 'data: [DONE]') continue;

            if (trimmed.startsWith('data: ')) {
              const jsonStr = trimmed.slice(6);
              try {
                const parsed = JSON.parse(jsonStr);
                // 서버가 usage를 실어 보내면 그게 토큰 수의 정확한 출처다.
                if (typeof parsed.usage?.completion_tokens === 'number') {
                  usageTokens = parsed.usage.completion_tokens;
                }
                // 마지막 청크의 값이 전체 생성 구간을 가장 잘 대표한다 — 계속 덮어쓴다.
                if (typeof parsed.timings?.predicted_per_second === 'number') {
                  serverTps = parsed.timings.predicted_per_second;
                }
                const delta = parsed.choices?.[0]?.delta?.content;
                if (delta) {
                  if (tFirstToken === null) tFirstToken = performance.now();
                  deltaChunks += 1;
                  accumulated += delta;
                  setMessages((prev) =>
                    prev.map((msg) =>
                      msg.id === assistantMsgId ? { ...msg, content: accumulated } : msg,
                    ),
                  );
                }
              } catch (e) {
                // 스트림 분할로 인한 JSON 파싱 생략
              }
            }
          }
        }

        // 남은 버퍼 처리
        if (buffer.trim() && buffer.trim().startsWith('data: ') && buffer.trim() !== 'data: [DONE]') {
          try {
            const parsed = JSON.parse(buffer.trim().slice(6));
            const delta = parsed.choices?.[0]?.delta?.content;
            if (delta) {
              accumulated += delta;
              setMessages((prev) =>
                prev.map((msg) =>
                  msg.id === assistantMsgId ? { ...msg, content: accumulated } : msg,
                ),
              );
            }
          } catch (e) {
            // ignore
          }
        }

        // 계측 확정. 토큰이 하나도 안 온 응답은 계측할 게 없으므로 perf를 붙이지 않는다 —
        // 0 tok/s를 표시하면 "느리다"로 읽히지만 실제로는 "측정 불가"다.
        if (tFirstToken !== null) {
          const genMs = performance.now() - tFirstToken;
          const tokens = usageTokens ?? deltaChunks;
          setMessages((prev) =>
            prev.map((msg) =>
              msg.id === assistantMsgId
                ? {
                    ...msg,
                    perf: {
                      ttftMs: Math.round(tFirstToken! - t0),
                      tokens,
                      // 서버 보고값이 있으면 그것이 정답이다. 없을 때만 클라이언트 근사 —
                      // 첫 토큰은 생성 구간 밖이므로 tokens-1을 genMs로 나눈다.
                      tokensPerSec:
                        serverTps !== null
                          ? Math.round(serverTps * 10) / 10
                          : genMs > 0 && tokens > 1
                            ? Math.round(((tokens - 1) / (genMs / 1000)) * 10) / 10
                            : 0,
                      exactTokens: usageTokens !== null,
                      serverTps: serverTps !== null,
                    },
                  }
                : msg,
            ),
          );
        }
      } else {
        // Non-streaming fallback
        const response = await fetch(endpoint, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            model: modelPath || 'default',
            messages: apiMessages,
            temperature,
            max_tokens: maxTokens,
            stream: false,
          }),
          signal: abortController.signal,
        });

        if (!response.ok) {
          const errText = await response.text();
          throw new Error(`서버 응답 오류 (${response.status}): ${errText}`);
        }

        const data = await response.json();
        const content = data.choices?.[0]?.message?.content || '(빈 응답)';

        setMessages((prev) =>
          prev.map((msg) => (msg.id === assistantMsgId ? { ...msg, content } : msg)),
        );
      }
    } catch (err: any) {
      if (err.name === 'AbortError') {
        setMessages((prev) =>
          prev.map((msg) =>
            msg.id === assistantMsgId
              ? { ...msg, content: msg.content + ' [사용자에 의해 중단됨]' }
              : msg,
          ),
        );
      } else {
        console.error('Chat error:', err);
        setMessages((prev) =>
          prev.map((msg) =>
            msg.id === assistantMsgId
              ? {
                  ...msg,
                  content: `⚠️ 오류 발생: ${err.message || 'mlx_lm.server 호출 중 예기치 않은 오류가 발생했습니다.'}`,
                }
              : msg,
          ),
        );
      }
    } finally {
      setLoading(false);
      abortControllerRef.current = null;
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  };

  const handleStop = () => {
    if (abortControllerRef.current) {
      abortControllerRef.current.abort();
    }
  };

  return (
    <div
      className="rounded-xl border border-hairline/12 bg-surface shadow-panel overflow-hidden flex flex-col h-[520px] min-w-0 w-full transition-all"
      onDragOver={(e) => { if (vlmActive) e.preventDefault(); }}
      onDrop={handleDrop}
    >
      {/* 1. Header */}
      <div className="px-4 py-3 bg-surfaceRaised border-b border-hairline/8 flex items-center justify-between shrink-0">
        <div className="flex items-center gap-2">
          <div className="w-8 h-8 rounded-lg bg-primary/10 flex items-center justify-center text-primary">
            <Sparkles className="w-4.5 h-4.5" />
          </div>
          <div>
            <div className="flex items-center gap-2">
              <h3 className="text-bodyStrong text-ink font-semibold">Interactive Chat Playground</h3>
              <span className="px-2 py-0.5 rounded-full bg-success/15 text-success text-[11px] font-medium flex items-center gap-1">
                <span className="w-1.5 h-1.5 rounded-full bg-success animate-pulse" />
                Live (Port {port})
              </span>
            </div>
            <p className="text-caption text-inkMuted text-[11px] truncate max-w-[28rem]">
              OpenAI 호환 API · {modelPath || 'MLX Model'}
              {adapterPath ? ` (${adapterPath.split('/').pop()})` : ''}
            </p>
          </div>
        </div>

        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={() => setShowSettings(!showSettings)}
            className={`p-2 rounded-lg text-caption flex items-center gap-1.5 transition-all border ${
              showSettings
                ? 'bg-primary/10 text-primary border-primary/30'
                : 'bg-surface hover:brightness-95 text-inkMuted border-hairline/8'
            }`}
            title="파라미터 및 RAG 설정"
          >
            <Sliders className="w-4 h-4" />
            <span className="hidden sm:inline">설정</span>
            {showSettings ? <ChevronUp className="w-3.5 h-3.5" /> : <ChevronDown className="w-3.5 h-3.5" />}
          </button>

          {messages.length > 0 && (
            <button
              type="button"
              onClick={handleClearHistory}
              className="p-2 rounded-lg bg-surface hover:bg-danger/10 text-inkMuted hover:text-danger border border-hairline/8 transition-all"
              title="대화 내역 초기화"
            >
              <Trash2 className="w-4 h-4" />
            </button>
          )}
        </div>
      </div>

      {/* 2. Controls / Settings Drawer */}
      {showSettings && (
        <div className="px-4 py-3 bg-surfaceRaised/60 border-b border-hairline/8 grid grid-cols-1 md:grid-cols-2 gap-3 text-caption shrink-0 animate-in fade-in duration-150">
          {/* System Prompt */}
          <div className="md:col-span-2">
            <label className="text-label uppercase text-inkFaint mb-1 block">System Prompt</label>
            <textarea
              rows={2}
              value={systemPrompt}
              onChange={(e) => setSystemPrompt(e.target.value)}
              className="w-full px-3 py-1.5 rounded-md bg-surface text-ink text-caption border border-hairline/8 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-primary resize-none"
            />
          </div>

          {/* Sliders & Switches */}
          <div className="space-y-2">
            <div>
              <div className="flex justify-between mb-1">
                <span className="text-inkFaint">Temperature: {temperature}</span>
                <span className="text-inkFaint text-[10px]">창의성/다양성</span>
              </div>
              <input
                type="range"
                min="0"
                max="2"
                step="0.1"
                value={temperature}
                onChange={(e) => setTemperature(parseFloat(e.target.value))}
                className="w-full accent-primary h-1.5 bg-surface rounded-lg appearance-none cursor-pointer"
              />
            </div>

            <div>
              <div className="flex justify-between mb-1">
                <span className="text-inkFaint">Max Tokens: {maxTokens}</span>
                <span className="text-inkFaint text-[10px]">최대 생성 길이</span>
              </div>
              <input
                type="range"
                min="64"
                max="2048"
                step="64"
                value={maxTokens}
                onChange={(e) => setMaxTokens(parseInt(e.target.value))}
                className="w-full accent-primary h-1.5 bg-surface rounded-lg appearance-none cursor-pointer"
              />
            </div>
          </div>

          <div className="space-y-2.5 pt-0.5">
            <div className="flex items-center justify-between bg-surface p-2 rounded-md border border-hairline/8">
              <span className="flex items-center gap-1.5 text-ink">
                <Zap className="w-3.5 h-3.5 text-warning" />
                <span>스트리밍 실시간 응답 (SSE)</span>
              </span>
              <input
                type="checkbox"
                checked={isStreaming}
                onChange={(e) => setIsStreaming(e.target.checked)}
                className="w-4 h-4 rounded accent-primary cursor-pointer"
              />
            </div>

            <div className="flex items-center justify-between bg-surface p-2 rounded-md border border-hairline/8">
              <div className="flex items-center gap-1.5 text-ink">
                <Database className="w-3.5 h-3.5 text-primary" />
                <span>LanceDB RAG 문맥 자동 주입</span>
              </div>
              <div className="flex items-center gap-2">
                {ragEnabled && (
                  <select
                    value={ragTopK}
                    onChange={(e) => setRagTopK(Number(e.target.value))}
                    className="px-1.5 py-0.5 bg-surfaceRaised text-ink text-[11px] rounded border border-hairline/8"
                    title="검색 문서 개수 (Top-K)"
                  >
                    <option value={2}>Top 2</option>
                    <option value={3}>Top 3</option>
                    <option value={5}>Top 5</option>
                  </select>
                )}
                <input
                  type="checkbox"
                  checked={ragEnabled}
                  onChange={(e) => setRagEnabled(e.target.checked)}
                  className="w-4 h-4 rounded accent-primary cursor-pointer"
                />
              </div>
            </div>
          </div>
        </div>
      )}

      {/* 3. Messages Chat Body */}
      <div className="flex-1 p-4 overflow-y-auto space-y-4 bg-surface/50 min-w-0">
        {messages.length === 0 ? (
          <div className="h-full flex flex-col items-center justify-center text-center p-6 space-y-4 min-w-0 overflow-hidden">
            <div className="w-12 h-12 rounded-full bg-primary/10 text-primary flex items-center justify-center">
              <Bot className="w-6 h-6" />
            </div>
            <div>
              <h4 className="text-bodyStrong text-ink font-semibold mb-1">
                로컬 MLX 챗 플레이그라운드가 준비되었습니다
              </h4>
              <p className="text-caption text-inkMuted max-w-[24rem]">
                질문을 입력하거나 아래 프롬프트 프리셋을 클릭하여 MLX 서빙 모델과의 대화를 시작하세요.
              </p>
            </div>

            {/* Presets */}
            <div className="grid grid-cols-2 gap-2 max-w-[28rem] w-full pt-2 min-w-0">
              {PRESET_PROMPTS.map((preset, idx) => (
                <button
                  key={idx}
                  type="button"
                  onClick={() => handleSend(preset.prompt)}
                  className="p-2.5 text-left rounded-lg bg-surfaceRaised hover:border-primary/40 border border-hairline/8 transition-all group"
                >
                  <div className="text-caption font-medium text-ink group-hover:text-primary transition-colors flex items-center justify-between">
                    <span>{preset.title}</span>
                    <Sparkles className="w-3 h-3 text-inkFaint group-hover:text-primary" />
                  </div>
                  <div className="text-[11px] text-inkMuted truncate mt-0.5">{preset.prompt}</div>
                </button>
              ))}
            </div>
          </div>
        ) : (
          messages.map((msg) => {
            const isUser = msg.role === 'user';
            const isAssistant = msg.role === 'assistant';

            return (
              <div
                key={msg.id}
                className={`flex gap-3 ${isUser ? 'justify-end' : 'justify-start'}`}
              >
                {!isUser && (
                  <div className="w-7 h-7 rounded-full bg-primary/10 text-primary flex items-center justify-center shrink-0 mt-0.5">
                    <Bot className="w-4 h-4" />
                  </div>
                )}

                <div className={`space-y-1.5 max-w-[82%] ${isUser ? 'items-end' : 'items-start'}`}>
                  {/* Bubble Container */}
                  <div
                    className={`p-3.5 rounded-2xl text-body leading-relaxed text-sm ${
                      isUser
                        ? 'bg-primary text-white rounded-tr-none shadow-sm'
                        : 'bg-surfaceRaised border border-hairline/8 text-ink rounded-tl-none shadow-sm'
                    }`}
                  >
                    {/* Assistant empty during load */}
                    {isAssistant && !msg.content && loading ? (
                      <div className="flex items-center gap-2 text-inkMuted py-0.5">
                        <Loader2 className="w-4 h-4 animate-spin text-primary" />
                        <span className="text-caption">응답 생성 중...</span>
                      </div>
                    ) : (
                      <div className="whitespace-pre-wrap break-words">{msg.content}</div>
                    )}
                  </div>

                  {/* RAG Context badge & inspection for User message */}
                  {isUser && msg.images && msg.images.length > 0 && (
                    <div className="flex gap-1.5 px-1">
                      {msg.images.map((url, i) => (
                        <img
                          key={i}
                          src={url}
                          alt={`전송 이미지 ${i + 1}`}
                          className="w-16 h-16 rounded-md object-cover border border-hairline/12"
                        />
                      ))}
                    </div>
                  )}

                  {isUser && msg.ragSources && msg.ragSources.length > 0 && (
                    <div className="flex flex-col items-end">
                      <button
                        type="button"
                        onClick={() => toggleSourceExpand(msg.id)}
                        className="px-2 py-0.5 rounded bg-primary/10 text-primary text-[11px] font-medium flex items-center gap-1 hover:bg-primary/20 transition-all border border-primary/20"
                      >
                        <Database className="w-3 h-3" />
                        <span>RAG 문맥 주입됨 ({msg.ragSources.length}건)</span>
                        {expandedSources[msg.id] ? (
                          <ChevronUp className="w-3 h-3" />
                        ) : (
                          <ChevronDown className="w-3 h-3" />
                        )}
                      </button>

                      {expandedSources[msg.id] && (
                        <div className="mt-1.5 p-2.5 rounded-lg bg-surfaceRaised border border-hairline/8 text-caption text-ink space-y-2 text-left max-w-[32rem] w-full animate-in fade-in duration-150">
                          <div className="text-[11px] font-semibold text-inkFaint uppercase flex items-center gap-1">
                            <FileText className="w-3 h-3 text-primary" />
                            LanceDB RAG 주입 검색 결과
                          </div>
                          {msg.ragSources.map((src, sIdx) => (
                            <div key={sIdx} className="p-2 rounded bg-surface border border-hairline/8 space-y-1 text-xs">
                              <div className="flex items-center justify-between text-inkMuted text-[11px]">
                                <span className="font-medium text-primary truncate max-w-[200px]">
                                  {src.filename || src.source || '문서'}
                                </span>
                                <span>거리 score: {src.score.toFixed(3)}</span>
                              </div>
                              <p className="text-ink text-caption leading-normal line-clamp-3">{src.text}</p>
                            </div>
                          ))}
                        </div>
                      )}
                    </div>
                  )}

                  {/* Assistant Copy action */}
                  {isAssistant && msg.content && (
                    <div className="flex items-center gap-2 text-[11px] text-inkFaint px-1">
                      <button
                        type="button"
                        onClick={() => handleCopy(msg.id, msg.content)}
                        className="flex items-center gap-1 hover:text-ink transition-colors"
                      >
                        {copiedId === msg.id ? (
                          <>
                            <Check className="w-3 h-3 text-success" />
                            <span className="text-success">복사됨</span>
                          </>
                        ) : (
                          <>
                            <Copy className="w-3 h-3" />
                            <span>복사</span>
                          </>
                        )}
                      </button>

                      {/* 서빙 품질 계측. 스트리밍 응답에만 붙는다 — 값이 없으면 아무것도
                          표시하지 않는다(0으로 채우면 "느림"으로 오독된다). */}
                      {msg.perf && (
                        <span className="flex items-center gap-2 text-inkFaint">
                          <span className="w-px h-3 bg-hairline/20" />
                          <span title="첫 토큰까지 걸린 시간 — 체감 반응성">
                            TTFT {msg.perf.ttftMs}ms
                          </span>
                          {msg.perf.tokensPerSec > 0 && (
                            <span
                              title={
                                msg.perf.serverTps
                                  ? '서버가 매 청크 보고한 timings.predicted_per_second 기준'
                                  : msg.perf.exactTokens
                                    ? '서버가 보고한 completion_tokens 기준'
                                    : '서버가 usage를 보내지 않아 스트림 청크 수로 근사한 값'
                              }
                            >
                              {msg.perf.tokensPerSec}{' '}
                              {msg.perf.serverTps || msg.perf.exactTokens ? 'tok/s' : 'tok/s≈'}
                            </span>
                          )}
                          <span>
                            {msg.perf.tokens}
                            {msg.perf.exactTokens ? ' 토큰' : ' 청크'}
                          </span>
                        </span>
                      )}
                    </div>
                  )}
                </div>

                {isUser && (
                  <div className="w-7 h-7 rounded-full bg-surfaceRaised border border-hairline/12 text-ink flex items-center justify-center shrink-0 mt-0.5">
                    <User className="w-4 h-4" />
                  </div>
                )}
              </div>
            );
          })
        )}
        <div ref={messagesEndRef} />
      </div>

      {/* RAG searching banner */}
      {ragSearching && (
        <div className="px-4 py-1.5 bg-primary/5 border-t border-hairline/8 flex items-center justify-center gap-2 text-caption text-primary shrink-0">
          <Database className="w-3.5 h-3.5 animate-bounce" />
          <span>LanceDB RAG에서 지식 베이스 검색 중...</span>
        </div>
      )}

      {/* 4. Input Bar */}
      <div className="p-3 bg-surfaceRaised border-t border-hairline/8 shrink-0">
        {vlmActive && pendingImages.length > 0 && (
          <div className="flex gap-2 mb-2">
            {pendingImages.map((url, i) => (
              <div key={i} className="relative">
                <img
                  src={url}
                  alt={`첨부 이미지 ${i + 1}`}
                  className="w-12 h-12 rounded-md object-cover border border-hairline/12"
                />
                <button
                  type="button"
                  onClick={() => setPendingImages((prev) => prev.filter((_, j) => j !== i))}
                  aria-label="첨부 제거"
                  className="absolute -top-1.5 -right-1.5 w-4 h-4 rounded-full bg-surface text-inkMuted hover:text-danger flex items-center justify-center text-[10px] shadow-panel"
                >
                  ✕
                </button>
              </div>
            ))}
          </div>
        )}
        <div className="flex gap-2 items-end">
          {vlmActive && (
            <>
              <input
                ref={fileInputRef}
                type="file"
                accept="image/*"
                multiple
                onChange={handlePickImages}
                className="hidden"
              />
              <button
                type="button"
                onClick={() => fileInputRef.current?.click()}
                disabled={loading || pendingImages.length >= MAX_ATTACHED_IMAGES}
                title={`이미지 첨부 (최대 ${MAX_ATTACHED_IMAGES}장)`}
                className="py-2.5 px-3 bg-surface hover:brightness-95 disabled:opacity-40 disabled:cursor-not-allowed text-inkMuted hover:text-ink rounded-lg border border-hairline/8 transition-all shrink-0 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
              >
                <ImagePlus className="w-4 h-4" />
              </button>
            </>
          )}
          <textarea
            ref={inputRef}
            rows={2}
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={handleKeyDown}
            onPaste={handlePaste}
            placeholder={
              loading
                ? '모델이 응답을 생성하는 중입니다...'
                : '메시지를 입력하세요 (Enter: 전송, Shift+Enter: 줄바꿈)'
            }
            disabled={loading}
            className="flex-1 px-3.5 py-2 rounded-lg bg-surface text-ink text-body placeholder:text-inkFaint border border-hairline/8 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary resize-none disabled:opacity-60"
          />

          {loading ? (
            <button
              type="button"
              onClick={handleStop}
              className="py-2.5 px-4 bg-dangerStrong hover:brightness-110 text-inverse font-medium rounded-lg transition-all flex items-center gap-1.5 shrink-0 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
              title="생성 중단"
            >
              <Square className="w-4 h-4 fill-current" />
              <span className="hidden sm:inline">중단</span>
            </button>
          ) : (
            <button
              type="button"
              onClick={() => handleSend()}
              disabled={!input.trim()}
              className="py-2.5 px-4 bg-primaryStrong hover:brightness-110 disabled:opacity-40 disabled:cursor-not-allowed text-inverse font-medium rounded-lg transition-all flex items-center gap-1.5 shrink-0 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
            >
              <Send className="w-4 h-4" />
              <span className="hidden sm:inline">전송</span>
            </button>
          )}
        </div>
      </div>
    </div>
  );
};
