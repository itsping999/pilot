(function () {
  "use strict";

  // --- i18n ---
  var I18N = {
    zh: {
      upload: "\u4e0a\u4f20",
      qa: "\u95ee\u7b54",
      checking: "\u68c0\u6d4b\u4e2d...",
      healthy: "\u6b63\u5e38",
      unhealthy: "\u5f02\u5e38",
      offline: "\u79bb\u7ebf",
      uploadDoc: "\u4e0a\u4f20\u6587\u6863",
      title: "\u6807\u9898",
      source: "\u6765\u6e90",
      file: "\u6587\u4ef6",
      content: "\u5185\u5bb9",
      titlePlaceholder: "\u6587\u6863\u6807\u9898",
      sourcePlaceholder: "\u4f8b\u5982 README.md \u6216 enterprise-handbook.md\uff08\u53ef\u9009\uff0c\u9ed8\u8ba4 local\uff09",
      contentPlaceholder: "\u5728\u6b64\u7c98\u8d34\u6587\u6863\u5185\u5bb9...",
      fileHint: "\u53ef\u9009 .md \u6216 .txt \u6587\u4ef6\uff0c\u9009\u62e9\u540e\u4f1a\u5165\u5e93\u6587\u4ef6\u5185\u5bb9\u3002",
      errTitleRequired: "\u8bf7\u586b\u5199\u6807\u9898",
      errContentRequired: "\u8bf7\u586b\u5199\u5185\u5bb9",
      uploading: "\u4e0a\u4f20\u4e2d...",
      docIndexed: "\u6587\u6863\u5df2\u5165\u5e93\uff1a",
      chunks: "\u4e2a\u5207\u7247",
      errPrefix: "\u9519\u8bef\uff1a",
      uploadFailed: "\u4e0a\u4f20\u5931\u8d25",
      sessionUploads: "\u5df2\u5165\u5e93\u6587\u6863",
      noUploads: "\u6682\u65e0\u5df2\u5165\u5e93\u6587\u6863",
      docsLoadFailed: "\u6587\u6863\u5217\u8868\u52a0\u8f7d\u5931\u8d25",
      deleteDoc: "\u5220\u9664",
      deleteFailed: "\u5220\u9664\u5931\u8d25",
      authRequired: "\u9700\u8981 API Token\uff0c\u8bf7\u5728\u6d4f\u89c8\u5668 localStorage \u4e2d\u8bbe\u7f6e kp_api_token",
      askQuestion: "\u63d0\u95ee",
      qaHint: "\u5148\u4e0a\u4f20\u6587\u6863\uff0c\u7136\u540e\u63d0\u95ee\u4ee5\u83b7\u53d6\u57fa\u4e8e\u5185\u5bb9\u7684\u56de\u7b54\u3002",
      inputPlaceholder: "\u8f93\u5165\u4f60\u7684\u95ee\u9898...",
      inputHint: "\u6309 Enter \u53d1\u9001\uff0cShift+Enter \u6362\u884c",
      thinking: "\u601d\u8003\u4e2d...",
      noAnswer: "\u672a\u751f\u6210\u56de\u7b54\u3002",
      queryFailed: "\u67e5\u8be2\u5931\u8d25",
      scoreTooltip: "\u5206\u6570: ",
      confidence: "\u7f6e\u4fe1\u5ea6",
      confidenceHigh: "\u9ad8",
      confidenceMedium: "\u4e2d",
      confidenceLow: "\u4f4e",
      references: "\u53c2\u8003\u6765\u6e90",
      sourceFile: "\u51fa\u5904\u6587\u4ef6",
      sourceHits: "\u547d\u4e2d",
      sourceHitUnit: "\u5904",
      sourceOpen: "\u70b9\u51fb\u67e5\u770b\u539f\u6587",
      originalText: "\u539f\u6587",
      close: "\u5173\u95ed",
      loadingDocument: "\u539f\u6587\u52a0\u8f7d\u4e2d...",
      documentLoadFailed: "\u539f\u6587\u52a0\u8f7d\u5931\u8d25",
      chunkDetails: "\u547d\u4e2d\u7247\u6bb5: ",
      langLabel: "EN",
    },
    en: {
      upload: "Upload",
      qa: "Q&A",
      checking: "Checking...",
      healthy: "Healthy",
      unhealthy: "Unhealthy",
      offline: "Offline",
      uploadDoc: "Upload Document",
      title: "Title",
      source: "Source",
      file: "File",
      content: "Content",
      titlePlaceholder: "Document title",
      sourcePlaceholder: "e.g. README.md or enterprise-handbook.md (optional, defaults to local)",
      contentPlaceholder: "Paste document content here...",
      fileHint: "Optional .md or .txt file. When selected, file content is indexed.",
      errTitleRequired: "Title is required",
      errContentRequired: "Content is required",
      uploading: "Uploading...",
      docIndexed: "Document indexed: ",
      chunks: "chunk(s)",
      errPrefix: "Error: ",
      uploadFailed: "Upload failed",
      sessionUploads: "Indexed Documents",
      noUploads: "No indexed documents yet.",
      docsLoadFailed: "Failed to load documents",
      deleteDoc: "Delete",
      deleteFailed: "Delete failed",
      authRequired: "API token required. Set kp_api_token in browser localStorage.",
      askQuestion: "Ask a Question",
      qaHint: "Upload documents first, then ask questions to get answers grounded in your content.",
      inputPlaceholder: "Type your question...",
      inputHint: "Press Enter to send, Shift+Enter for new line",
      thinking: "Thinking...",
      noAnswer: "No answer generated.",
      queryFailed: "Query failed",
      scoreTooltip: "Score: ",
      confidence: "Confidence",
      confidenceHigh: "High",
      confidenceMedium: "Medium",
      confidenceLow: "Low",
      references: "References",
      sourceFile: "Source file",
      sourceHits: "Hits",
      sourceHitUnit: "",
      sourceOpen: "Click to view original",
      originalText: "Original Text",
      close: "Close",
      loadingDocument: "Loading original text...",
      documentLoadFailed: "Failed to load original text",
      chunkDetails: "Matched chunks: ",
      langLabel: "\u4e2d\u6587",
    },
  };

  var currentLang = localStorage.getItem("kp_lang") || "zh";

  function t(key) {
    return (I18N[currentLang] && I18N[currentLang][key]) || I18N.zh[key] || key;
  }

  function applyI18n() {
    document.documentElement.lang = currentLang === "zh" ? "zh-CN" : "en";

    var els = document.querySelectorAll("[data-i18n]");
    for (var i = 0; i < els.length; i++) {
      els[i].textContent = t(els[i].getAttribute("data-i18n"));
    }

    var phEls = document.querySelectorAll("[data-i18n-placeholder]");
    for (var j = 0; j < phEls.length; j++) {
      phEls[j].placeholder = t(phEls[j].getAttribute("data-i18n-placeholder"));
    }

    var htmlEls = document.querySelectorAll("[data-i18n-html]");
    for (var k = 0; k < htmlEls.length; k++) {
      var el = htmlEls[k];
      var iconSvg = el.querySelector("svg");
      if (iconSvg) {
        el.innerHTML = iconSvg.outerHTML + " " + t(el.getAttribute("data-i18n-html"));
      } else {
        el.textContent = t(el.getAttribute("data-i18n-html"));
      }
    }

    // Language toggle labels (both desktop and mobile)
    var langBtns = document.querySelectorAll(".lang-btn");
    for (var l = 0; l < langBtns.length; l++) {
      langBtns[l].textContent = t("langLabel");
    }

    renderHistory();
  }

  function switchLang() {
    currentLang = currentLang === "zh" ? "en" : "zh";
    localStorage.setItem("kp_lang", currentLang);
    applyI18n();
  }

  // --- Theme ---
  var THEME_KEY = "kp_theme";

  function getSystemTheme() {
    return window.matchMedia && window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  }

  function getStoredTheme() {
    return localStorage.getItem(THEME_KEY);
  }

  function getActiveTheme() {
    return getStoredTheme() || getSystemTheme();
  }

  function applyTheme(theme) {
    document.documentElement.setAttribute("data-theme", theme);
    // Toggle sun/moon icons in all theme toggle buttons
    var suns = document.querySelectorAll(".icon-sun");
    var moons = document.querySelectorAll(".icon-moon");
    var isDark = theme === "dark";
    for (var i = 0; i < suns.length; i++) suns[i].style.display = isDark ? "none" : "block";
    for (var j = 0; j < moons.length; j++) moons[j].style.display = isDark ? "block" : "none";
  }

  function toggleTheme() {
    var next = getActiveTheme() === "dark" ? "light" : "dark";
    localStorage.setItem(THEME_KEY, next);
    applyTheme(next);
  }

  // Apply theme early to avoid flash
  applyTheme(getActiveTheme());

  // Listen for system theme changes
  if (window.matchMedia) {
    window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", function () {
      if (!getStoredTheme()) applyTheme(getSystemTheme());
    });
  }

  // --- Health check ---
  async function checkHealth() {
    var dot = document.querySelector("#health-status .status-dot");
    var text = document.querySelector("#health-status .status-text");
    if (!dot || !text) return;
    try {
      var res = await fetch("/health");
      if (res.ok) {
        dot.className = "status-dot ok";
        text.setAttribute("data-i18n", "healthy");
        text.textContent = t("healthy");
      } else {
        dot.className = "status-dot err";
        text.setAttribute("data-i18n", "unhealthy");
        text.textContent = t("unhealthy");
      }
    } catch (_e) {
      dot.className = "status-dot err";
      text.setAttribute("data-i18n", "offline");
      text.textContent = t("offline");
    }
  }

  // --- Page navigation ---
  var mobileLinks = document.querySelectorAll(".mobile-nav-panel .nav-link");

  function markActiveNav() {
    var page = document.body.getAttribute("data-page") || "upload";

    var allLinks = document.querySelectorAll(".nav-link[data-view]");
    for (var i = 0; i < allLinks.length; i++) {
      allLinks[i].classList.toggle("active", allLinks[i].dataset.view === page);
    }
  }

  // --- Hamburger / mobile menu ---
  var hamburger = document.getElementById("hamburger");
  var mobileOverlay = document.getElementById("mobile-overlay");

  function closeMobileMenu() {
    if (!mobileOverlay) return;
    mobileOverlay.classList.remove("open");
  }

  function toggleMobileMenu() {
    if (!mobileOverlay) return;
    mobileOverlay.classList.toggle("open");
  }

  if (hamburger) {
    hamburger.addEventListener("click", function (e) {
      e.stopPropagation();
      toggleMobileMenu();
    });
  }

  if (mobileOverlay) {
    mobileOverlay.addEventListener("click", function (e) {
      if (e.target === mobileOverlay) closeMobileMenu();
    });
  }

  // Mobile nav link clicks close the menu
  for (var m = 0; m < mobileLinks.length; m++) {
    mobileLinks[m].addEventListener("click", closeMobileMenu);
  }

  // --- Upload form ---
  var form = document.getElementById("upload-form");
  var titleInput = document.getElementById("doc-title");
  var sourceInput = document.getElementById("doc-source");
  var fileInput = document.getElementById("doc-file");
  var textInput = document.getElementById("doc-text");
  var uploadBtn = document.getElementById("upload-btn");
  var resultPanel = document.getElementById("upload-result");
  var historyEl = document.getElementById("upload-history");
  var errorTitle = document.getElementById("error-title");
  var errorText = document.getElementById("error-text");

  function showError(el, msg) {
    el.textContent = msg;
    el.classList.add("visible");
  }
  function clearError(el) {
    el.textContent = "";
    el.classList.remove("visible");
  }

  async function renderHistory() {
    if (!historyEl) return;
    var list = [];
    try {
      var res = await fetch("/documents", { headers: authHeaders() });
      var data = await parseJsonResponse(res);
      if (!res.ok) throw new Error(data.error || t("docsLoadFailed"));
      list = data.documents || [];
    } catch (err) {
      historyEl.innerHTML = '<p class="empty-state">' + esc(t("docsLoadFailed") + ": " + String(err.message || err)) + "</p>";
      return;
    }

    if (list.length === 0) {
      historyEl.innerHTML = '<p class="empty-state">' + t("noUploads") + "</p>";
      return;
    }
    historyEl.innerHTML = list
      .slice()
      .reverse()
      .map(function (item) {
        return (
          '<div class="history-item">' +
            '<div class="history-item-left">' +
              '<span class="history-title">' + esc(item.title) + "</span>" +
              '<span class="history-id">' + esc(item.id) + "</span>" +
            "</div>" +
            '<div class="history-actions">' +
              '<span class="history-badge">' + item.chunks + " " + t("chunks") + "</span>" +
              '<button class="history-delete" type="button" data-delete-doc="' + esc(item.id) + '">' + t("deleteDoc") + "</button>" +
            "</div>" +
          "</div>"
        );
      })
      .join("");
  }

  if (historyEl) {
    historyEl.addEventListener("click", async function (event) {
      var button = event.target.closest("[data-delete-doc]");
      if (!button) return;

      var id = button.getAttribute("data-delete-doc");
      button.disabled = true;
      try {
        var res = await fetch("/documents/" + encodeURIComponent(id), {
          method: "DELETE",
          headers: authHeaders(),
        });
        var data = await parseJsonResponse(res);
        if (!res.ok) throw new Error(data.error || t("deleteFailed"));
        await renderHistory();
      } catch (err) {
        button.disabled = false;
        resultPanel.className = "result-panel error";
        resultPanel.innerHTML = "<strong>" + t("errPrefix") + "</strong> " + esc(String(err.message || err));
      }
    });
  }

  function esc(s) {
    var d = document.createElement("div");
    d.textContent = s;
    return d.innerHTML;
  }

  function authHeaders(extra) {
    var headers = extra || {};
    var token = localStorage.getItem("kp_api_token");
    if (token) headers.Authorization = "Bearer " + token;
    return headers;
  }

  async function parseJsonResponse(res) {
    var body = await res.json();
    if (!res.ok) {
      return { error: body.message || body.error || (res.status === 401 ? t("authRequired") : res.statusText) };
    }
    return body.data || body;
  }

  var uploadSvg =
    '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="18" height="18">' +
      '<path d="M21 15v4a2 2 0 01-2 2H5a2 2 0 01-2-2v-4"/>' +
      '<polyline points="17,8 12,3 7,8"/>' +
      '<line x1="12" y1="3" x2="12" y2="15"/>' +
    "</svg> ";

  if (form) {
    form.addEventListener("submit", async function (e) {
      e.preventDefault();
      var valid = true;

      var selectedFile = fileInput && fileInput.files && fileInput.files[0];
      if (!titleInput.value.trim() && !selectedFile) { showError(errorTitle, t("errTitleRequired")); valid = false; } else { clearError(errorTitle); }
      if (!textInput.value.trim() && !selectedFile) { showError(errorText, t("errContentRequired")); valid = false; } else { clearError(errorText); }
      if (!valid) return;

      uploadBtn.disabled = true;
      uploadBtn.innerHTML = uploadSvg + t("uploading");
      resultPanel.classList.add("hidden");

      try {
        var title = titleInput.value.trim();
        var source = sourceInput.value.trim();
        var res;
        if (selectedFile) {
          var formData = new FormData();
          if (title) formData.append("title", title);
          if (source) formData.append("source", source);
          formData.append("file", selectedFile);
          res = await fetch("/documents/upload", {
            method: "POST",
            headers: authHeaders(),
            body: formData,
          });
        } else {
          var body = {
            title: title,
            text: textInput.value.trim(),
          };
          if (source) body.source = source;

          res = await fetch("/documents", {
            method: "POST",
            headers: authHeaders({ "Content-Type": "application/json" }),
            body: JSON.stringify(body),
          });
        }
        var data = await parseJsonResponse(res);

        if (!res.ok) {
          resultPanel.className = "result-panel error";
          resultPanel.innerHTML = "<strong>" + t("errPrefix") + "</strong> " + esc(data.error || t("uploadFailed"));
          return;
        }

        resultPanel.className = "result-panel success";
        resultPanel.innerHTML =
          "<strong>" + t("docIndexed") + "</strong> " +
          esc(data.id) + " \u2014 " +
          data.chunks + " " + t("chunks");

        await renderHistory();

        titleInput.value = "";
        sourceInput.value = "";
        if (fileInput) fileInput.value = "";
        textInput.value = "";
        clearError(errorTitle);
        clearError(errorText);
      } catch (err) {
        resultPanel.className = "result-panel error";
        resultPanel.innerHTML = "<strong>" + t("errPrefix") + "</strong> " + esc(String(err));
      } finally {
        uploadBtn.disabled = false;
        uploadBtn.innerHTML = uploadSvg + t("uploadDoc");
      }
    });
  }

  // --- Q&A Chat ---
  var chatMessages = document.getElementById("chat-messages");
  var chatInput = document.getElementById("chat-input");
  var chatSend = document.getElementById("chat-send");

  if (chatInput && chatSend) {
    chatInput.addEventListener("input", function () {
      chatSend.disabled = !chatInput.value.trim();
      chatInput.style.height = "auto";
      chatInput.style.height = Math.min(chatInput.scrollHeight, 120) + "px";
    });

    chatInput.addEventListener("keydown", function (e) {
      if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        if (chatInput.value.trim()) sendQuestion();
      }
    });

    chatSend.addEventListener("click", function () {
      if (chatInput.value.trim()) sendQuestion();
    });
  }

  function appendMessage(role, content) {
    var welcome = chatMessages.querySelector(".chat-welcome");
    if (welcome) welcome.remove();

    var msg = document.createElement("div");
    msg.className = "message " + role;
    var bubble = document.createElement("div");
    bubble.className = "message-bubble";
    if (typeof content === "string") {
      bubble.textContent = content;
    } else {
      bubble.appendChild(content);
    }
    msg.appendChild(bubble);
    chatMessages.appendChild(msg);
    chatMessages.scrollTop = chatMessages.scrollHeight;
    return msg;
  }

  function appendEmptyMessage(role) {
    var welcome = chatMessages.querySelector(".chat-welcome");
    if (welcome) welcome.remove();

    var msg = document.createElement("div");
    msg.className = "message " + role;
    var bubble = document.createElement("div");
    bubble.className = "message-bubble";
    msg.appendChild(bubble);
    chatMessages.appendChild(msg);
    chatMessages.scrollTop = chatMessages.scrollHeight;
    return msg;
  }

  function typeMessage(msg, text) {
    var bubble = msg.querySelector(".message-bubble");
    var chars = Array.from(text);
    var index = 0;
    var delay = window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches ? 0 : 18;

    msg.classList.add("typing");
    bubble.textContent = "";

    return new Promise(function (resolve) {
      function renderNext() {
        if (index >= chars.length) {
          msg.classList.remove("typing");
          resolve();
          return;
        }

        bubble.textContent += chars[index];
        index += 1;
        chatMessages.scrollTop = chatMessages.scrollHeight;

        if (delay === 0) {
          renderNext();
        } else {
          window.setTimeout(renderNext, delay);
        }
      }

      renderNext();
    });
  }

  function appendCitations(parentEl, citations) {
    if (!citations || citations.length === 0) return;
    var wrap = document.createElement("div");
    wrap.className = "citations";

    var title = document.createElement("div");
    title.className = "citations-title";
    title.textContent = t("references");
    wrap.appendChild(title);

    var groups = groupCitations(citations);
    groups.forEach(function (group) {
      var row = document.createElement("button");
      row.className = "citation-source";
      row.type = "button";
      row.title = citationDetailsTitle(group.refs);
      row.addEventListener("click", function () {
        openSourceDocument(group.documentId, group.refs);
      });

      var label = document.createElement("div");
      label.className = "citation-source-label";
      label.textContent = t("sourceFile");
      row.appendChild(label);

      var sourceName = document.createElement("div");
      sourceName.className = "citation-source-name";
      sourceName.textContent = group.title || sourceFileName(group.source);
      row.appendChild(sourceName);

      var meta = document.createElement("div");
      meta.className = "citation-source-meta";
      meta.textContent = sourceMeta(group);
      row.appendChild(meta);

      wrap.appendChild(row);
    });

    parentEl.querySelector(".message-bubble").appendChild(wrap);
  }

  function appendConfidence(parentEl, confidence) {
    if (!confidence) return;
    var bubble = parentEl.querySelector(".message-bubble");
    if (!bubble) return;

    var level = confidence.level || "medium";
    var badge = document.createElement("div");
    badge.className = "confidence confidence-" + level;

    var label = document.createElement("span");
    label.className = "confidence-label";
    label.textContent = t("confidence") + ": " + confidenceLabel(level);
    badge.appendChild(label);

    if (typeof confidence.score === "number") {
      var score = document.createElement("span");
      score.className = "confidence-score";
      score.textContent = confidence.score.toFixed(3);
      badge.appendChild(score);
    }

    bubble.appendChild(badge);
  }

  function confidenceLabel(level) {
    if (level === "high") return t("confidenceHigh");
    if (level === "low") return t("confidenceLow");
    return t("confidenceMedium");
  }

  function groupCitations(citations) {
    var groups = [];
    var bySource = {};

    citations.forEach(function (c) {
      var source = c.source || c.document_id || c.chunk_id || t("references");
      var documentId = c.document_id || "";
      var groupKey = documentId + "::" + source;
      if (!bySource[groupKey]) {
        bySource[groupKey] = {
          documentId: documentId,
          source: source,
          title: c.document_title || "",
          sections: [],
          refs: [],
          seen: {}
        };
        groups.push(bySource[groupKey]);
      }
      if (c.document_title && !bySource[groupKey].title) bySource[groupKey].title = c.document_title;
      if (c.section && bySource[groupKey].sections.indexOf(c.section) === -1) {
        bySource[groupKey].sections.push(c.section);
      }
      var key = c.chunk_id || "";
      if (!bySource[groupKey].seen[key]) {
        bySource[groupKey].seen[key] = true;
        bySource[groupKey].refs.push(c);
      }
    });

    groups.forEach(function (group) {
      group.refs.sort(function (left, right) {
        return citationIndex(left) - citationIndex(right);
      });
    });

    return groups;
  }

  function sourceFileName(source) {
    var value = String(source || "").trim();
    if (!value) return t("sourceFile");
    var parts = value.split(/[\\/]/).filter(Boolean);
    return parts.length ? parts[parts.length - 1] : value;
  }

  function sourceMeta(group) {
    var value = String(group.source || "").trim();
    var fileName = sourceFileName(value);
    var meta = [];
    if (value && value !== fileName) meta.push(value);
    if (group.sections && group.sections.length) meta.push(group.sections.slice(0, 3).join(" / "));
    meta.push(t("sourceHits") + " " + group.refs.length + " " + t("sourceHitUnit"));
    meta.push(t("sourceOpen"));
    return meta.join(" · ");
  }

  function citationDetailsTitle(refs) {
    return t("chunkDetails") + refs.map(function (c) {
      var section = c.section ? " · " + c.section : "";
      return c.chunk_id + section + " (" + t("scoreTooltip") + (c.score || 0).toFixed(3) + ")";
    }).join(", ");
  }

  function citationIndex(c) {
    var id = c.chunk_id || "";
    var parts = id.split(":");
    return parts.length > 1 ? Number(parts[parts.length - 1]) : Number.POSITIVE_INFINITY;
  }

  async function openSourceDocument(documentId, refs) {
    if (!documentId) return;
    var modal = ensureSourceModal();
    var title = modal.querySelector(".source-modal-title");
    var meta = modal.querySelector(".source-modal-meta");
    var body = modal.querySelector(".source-modal-body");

    title.textContent = t("originalText");
    meta.textContent = "";
    body.textContent = t("loadingDocument");
    modal.classList.add("open");

    try {
      var res = await fetch("/documents/" + encodeURIComponent(documentId), {
        headers: authHeaders(),
      });
      var data = await parseJsonResponse(res);
      if (!res.ok) throw new Error(data.error || t("documentLoadFailed"));

      title.textContent = data.title || documentId;
      meta.textContent = data.source || documentId;
      var highlightTexts = await loadCitationHighlightTexts(documentId, refs || []);
      renderSourceText(body, data.text || "", highlightTexts);
    } catch (err) {
      body.textContent = t("errPrefix") + String(err.message || err);
    }
  }

  async function loadCitationHighlightTexts(documentId, refs) {
    var citedIds = {};
    refs.forEach(function (ref) {
      if (ref.chunk_id) citedIds[ref.chunk_id] = true;
    });
    if (Object.keys(citedIds).length === 0) return [];

    try {
      var res = await fetch("/documents/" + encodeURIComponent(documentId) + "/chunks", {
        headers: authHeaders(),
      });
      var data = await parseJsonResponse(res);
      if (!res.ok || !data.chunks) return [];
      return data.chunks
        .filter(function (chunk) {
          return citedIds[chunk.id];
        })
        .map(function (chunk) {
          return compactWhitespace(chunk.text || "");
        })
        .filter(Boolean);
    } catch (_err) {
      return [];
    }
  }

  function renderSourceText(body, text, highlightTexts) {
    body.innerHTML = "";
    var normalizedText = String(text || "");
    var ranges = sourceHighlightRanges(normalizedText, highlightTexts);

    if (ranges.length === 0) {
      body.textContent = normalizedText;
      return;
    }

    var cursor = 0;
    ranges.forEach(function (range, index) {
      if (range.start > cursor) {
        body.appendChild(document.createTextNode(normalizedText.slice(cursor, range.start)));
      }

      var mark = document.createElement("mark");
      mark.className = "source-hit";
      if (index === 0) mark.id = "source-hit-first";
      mark.textContent = normalizedText.slice(range.start, range.end);
      body.appendChild(mark);
      cursor = range.end;
    });

    if (cursor < normalizedText.length) {
      body.appendChild(document.createTextNode(normalizedText.slice(cursor)));
    }

    var firstHit = body.querySelector("#source-hit-first");
    if (firstHit) {
      window.setTimeout(function () {
        firstHit.scrollIntoView({ block: "center" });
      }, 50);
    }
  }

  function sourceHighlightRanges(text, highlightTexts) {
    var ranges = [];
    var searchable = normalizedSearchText(text);
    highlightTexts.forEach(function (highlightText) {
      var candidates = highlightCandidates(highlightText);
      for (var i = 0; i < candidates.length; i++) {
        var match = normalizedRange(searchable, candidates[i]);
        if (!match) continue;
        var start = match.start;
        var end = match.end;
        if (!ranges.some(function (range) {
          return start < range.end && end > range.start;
        })) {
          ranges.push({ start: start, end: end });
        }
        break;
      }
    });

    ranges.sort(function (left, right) {
      return left.start - right.start;
    });
    return ranges;
  }

  function normalizedSearchText(text) {
    var normalized = "";
    var map = [];
    var lastWasSpace = false;
    for (var i = 0; i < text.length; i++) {
      var ch = text.charAt(i);
      if (/\s/.test(ch)) {
        if (!lastWasSpace) {
          normalized += " ";
          map.push(i);
          lastWasSpace = true;
        }
      } else {
        normalized += ch;
        map.push(i);
        lastWasSpace = false;
      }
    }

    return { text: normalized, map: map };
  }

  function normalizedRange(searchable, candidate) {
    var normalizedCandidate = compactWhitespace(candidate);
    var start = searchable.text.indexOf(normalizedCandidate);
    if (start === -1) return null;

    var lastNormalizedIndex = start + normalizedCandidate.length - 1;
    var originalStart = searchable.map[start];
    var originalEnd = searchable.map[lastNormalizedIndex] + 1;
    if (originalStart == null || originalEnd == null || originalEnd <= originalStart) return null;
    return { start: originalStart, end: originalEnd };
  }

  function highlightCandidates(text) {
    var compact = compactWhitespace(text);
    var candidates = [];
    if (compact.length >= 16) candidates.push(compact);

    var sentences = compact.split(/(?<=[。！？.!?])/).map(compactWhitespace).filter(function (value) {
      return value.length >= 16;
    });
    sentences.slice(0, 3).forEach(function (sentence) {
      candidates.push(sentence);
    });

    if (compact.length > 120) candidates.push(compact.slice(0, 120));
    if (compact.length > 80) candidates.push(compact.slice(0, 80));
    return candidates.filter(function (value, index) {
      return value && candidates.indexOf(value) === index;
    });
  }

  function compactWhitespace(text) {
    return String(text || "").replace(/\s+/g, " ").trim();
  }

  function ensureSourceModal() {
    var existing = document.getElementById("source-modal");
    if (existing) return existing;

    var overlay = document.createElement("div");
    overlay.id = "source-modal";
    overlay.className = "source-modal";

    var panel = document.createElement("div");
    panel.className = "source-modal-panel";

    var header = document.createElement("div");
    header.className = "source-modal-header";

    var heading = document.createElement("div");
    var title = document.createElement("h3");
    title.className = "source-modal-title";
    var meta = document.createElement("p");
    meta.className = "source-modal-meta";
    heading.appendChild(title);
    heading.appendChild(meta);

    var close = document.createElement("button");
    close.className = "source-modal-close";
    close.type = "button";
    close.textContent = t("close");
    close.addEventListener("click", closeSourceModal);

    header.appendChild(heading);
    header.appendChild(close);

    var body = document.createElement("pre");
    body.className = "source-modal-body";

    panel.appendChild(header);
    panel.appendChild(body);
    overlay.appendChild(panel);

    overlay.addEventListener("click", function (event) {
      if (event.target === overlay) closeSourceModal();
    });

    document.body.appendChild(overlay);
    return overlay;
  }

  function closeSourceModal() {
    var modal = document.getElementById("source-modal");
    if (modal) modal.classList.remove("open");
  }

  document.addEventListener("keydown", function (event) {
    if (event.key === "Escape") closeSourceModal();
  });

  function showLoading() {
    var content = document.createElement("div");
    content.className = "loading-dots";
    content.innerHTML = "<span></span><span></span><span></span>";
    return appendMessage("assistant", content);
  }

  async function sendQuestion() {
    var question = chatInput.value.trim();
    if (!question) return;

    appendMessage("user", question);
    chatInput.value = "";
    chatInput.style.height = "auto";
    chatSend.disabled = true;

    var loadingMsg = showLoading();

    try {
      var res = await fetch("/bot/chat/stream", {
        method: "POST",
        headers: authHeaders({ "Content-Type": "application/json" }),
        body: JSON.stringify({ message: question, top_k: 5 }),
      });

      if (!res.ok) {
        loadingMsg.remove();
        var data = await parseJsonResponse(res);
        appendMessage("error", t("errPrefix") + (data.error || t("queryFailed")));
        return;
      }

      var msg = appendEmptyMessage("assistant");
      var bubble = msg.querySelector(".message-bubble");
      msg.classList.add("typing");
      bubble.textContent = "";
      loadingMsg.remove();

      var badgeEl = document.createElement("div");
      badgeEl.className = "bot-action-badge";
      badgeEl.style.fontSize = "0.75rem";
      badgeEl.style.opacity = "0.75";
      badgeEl.style.marginBottom = "4px";
      bubble.appendChild(badgeEl);

      var textEl = document.createElement("div");
      bubble.appendChild(textEl);

      var reader = res.body.getReader();
      var decoder = new TextDecoder();
      var buffer = "";
      var metadata = null;

      while (true) {
        var result = await reader.read();
        if (result.done) break;
        
        buffer += decoder.decode(result.value, { stream: true });
        var lines = buffer.split("\n");
        buffer = lines.pop() || "";
        
        for (var i = 0; i < lines.length; i++) {
          var line = lines[i].trim();
          if (!line.startsWith("data: ")) continue;
          
          var jsonStr = line.slice(6);
          try {
            var event = JSON.parse(jsonStr);
            if (event.type === "action") {
              if (event.action === "skill") {
                badgeEl.textContent = "⚡ 执行数据技能: " + (event.skill_name || "");
              } else if (event.action === "rag") {
                badgeEl.textContent = "📖 知识库检索";
              }
            } else if (event.type === "metadata") {
              metadata = event;
            } else if (event.type === "token") {
              textEl.textContent += event.content;
              chatMessages.scrollTop = chatMessages.scrollHeight;
            } else if (event.type === "done") {
              // Stream complete
            }
          } catch (e) {
            // Skip malformed JSON
          }
        }
      }

      msg.classList.remove("typing");
      if (metadata) {
        appendConfidence(msg, metadata.confidence);
        appendCitations(msg, metadata.citations);
      }
    } catch (err) {
      loadingMsg.remove();
      appendMessage("error", t("errPrefix") + String(err));
    } finally {
      chatSend.disabled = !chatInput.value.trim();
    }
  }

  // --- Init ---
  // Theme toggle (both desktop and mobile)
  var themeBtns = document.querySelectorAll("#theme-toggle, .mobile-theme-btn");
  for (var tb = 0; tb < themeBtns.length; tb++) {
    themeBtns[tb].addEventListener("click", toggleTheme);
  }

  // Language toggle (both desktop and mobile)
  var langBtns = document.querySelectorAll("#lang-toggle, .mobile-lang-btn");
  for (var lb = 0; lb < langBtns.length; lb++) {
    langBtns[lb].addEventListener("click", switchLang);
  }

  applyI18n();
  markActiveNav();
  checkHealth();
  renderHistory();
})();
