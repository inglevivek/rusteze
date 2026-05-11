// State
let filesToUpload = [];
let isProcessing = false;

// DOM Elements
const leftPanel = document.getElementById('leftPanel');
const rightPanel = document.getElementById('rightPanel');
const leftResizeHandle = document.getElementById('leftResizeHandle');
const rightResizeHandle = document.getElementById('rightResizeHandle');
const toggleRightPanelBtn = document.getElementById('toggleRightPanelBtn');

const uploadZone = document.getElementById('uploadZone');
const documentUpload = document.getElementById('documentUpload');
const uploadQueue = document.getElementById('uploadQueue');
const processBatchBtn = document.getElementById('processBatchBtn');
const caseIdInput = document.getElementById('caseIdInput');
const caseList = document.getElementById('caseList');

const chatInput = document.getElementById('chatInput');
const sendChatBtn = document.getElementById('sendChatBtn');
const chatHistory = document.getElementById('chatHistory');
const emptyState = document.getElementById('emptyState');
const renderToggle = document.getElementById('renderToggle');

const caseDetailsContent = document.getElementById('caseDetailsContent');
const caseDetailsEmpty = document.getElementById('caseDetailsEmpty');
const reportOutput = document.getElementById('reportOutput');
const documentTextArea = document.getElementById('documentTextArea');

// New Buttons
const clearHistoryBtn = document.getElementById('clearHistoryBtn');
const loadOlderBtn = document.getElementById('loadOlderBtn');
const loadOlderContainer = document.getElementById('loadOlderContainer');
const readjudicateBtn = document.getElementById('readjudicateBtn');
const deleteCaseBtn = document.getElementById('deleteCaseBtn');
const updateTextBtn = document.getElementById('updateTextBtn');

// State for pagination
let oldestMessageId = null;
let historyLoading = false;

// Initialization
window.onload = () => {
    loadCases();
    setupResizers();
    setupDragAndDrop();
    setupChatInput();
    setupActionButtons();
    
    // Marked configuration
    marked.setOptions({
        breaks: true,
        gfm: true
    });
};

// Panel Resizing Logic
function setupResizers() {
    let isResizingLeft = false;
    let isResizingRight = false;

    leftResizeHandle.addEventListener('mousedown', (e) => {
        isResizingLeft = true;
        leftResizeHandle.classList.add('active');
        document.body.style.cursor = 'col-resize';
        e.preventDefault();
    });

    rightResizeHandle.addEventListener('mousedown', (e) => {
        isResizingRight = true;
        rightResizeHandle.classList.add('active');
        document.body.style.cursor = 'col-resize';
        e.preventDefault();
    });

    document.addEventListener('mousemove', (e) => {
        if (!isResizingLeft && !isResizingRight) return;

        if (isResizingLeft) {
            const newWidth = Math.max(250, Math.min(e.clientX, 600));
            leftPanel.style.width = `${newWidth}px`;
        }

        if (isResizingRight) {
            const containerWidth = document.body.clientWidth;
            const newWidth = Math.max(250, Math.min(containerWidth - e.clientX, 600));
            rightPanel.style.width = `${newWidth}px`;
        }
    });

    document.addEventListener('mouseup', () => {
        if (isResizingLeft) {
            isResizingLeft = false;
            leftResizeHandle.classList.remove('active');
        }
        if (isResizingRight) {
            isResizingRight = false;
            rightResizeHandle.classList.remove('active');
        }
        document.body.style.cursor = 'default';
    });

    toggleRightPanelBtn.addEventListener('click', () => {
        if (rightPanel.style.display === 'none') {
            rightPanel.style.display = 'flex';
            rightResizeHandle.style.display = 'block';
        } else {
            rightPanel.style.display = 'none';
            rightResizeHandle.style.display = 'none';
        }
    });
}

// Drag & Drop Logic
function setupDragAndDrop() {
    ['dragenter', 'dragover', 'dragleave', 'drop'].forEach(eventName => {
        uploadZone.addEventListener(eventName, preventDefaults, false);
    });

    function preventDefaults(e) {
        e.preventDefault();
        e.stopPropagation();
    }

    ['dragenter', 'dragover'].forEach(eventName => {
        uploadZone.addEventListener(eventName, () => {
            uploadZone.classList.add('border-blue-500', 'bg-blue-900/20');
        }, false);
    });

    ['dragleave', 'drop'].forEach(eventName => {
        uploadZone.addEventListener(eventName, () => {
            uploadZone.classList.remove('border-blue-500', 'bg-blue-900/20');
        }, false);
    });

    uploadZone.addEventListener('drop', (e) => {
        handleFiles(e.dataTransfer.files);
    }, false);

    uploadZone.addEventListener('click', () => {
        documentUpload.click();
    });

    documentUpload.addEventListener('change', function() {
        handleFiles(this.files);
        // Reset input to allow selecting the same file again if needed
        this.value = null; 
    });
}

function handleFiles(files) {
    if (isProcessing) return;
    
    for (let i = 0; i < files.length; i++) {
        filesToUpload.push({
            file: files[i],
            id: 'file-' + Date.now() + '-' + i,
            status: 'queued',
            error: null
        });
    }
    
    renderQueue();
}

function removeFile(id) {
    if (isProcessing) return;
    filesToUpload = filesToUpload.filter(f => f.id !== id);
    renderQueue();
}

function renderQueue() {
    uploadQueue.innerHTML = '';
    
    if (filesToUpload.length === 0) {
        processBatchBtn.classList.add('hidden');
        return;
    }
    
    processBatchBtn.classList.remove('hidden');
    
    filesToUpload.forEach(fileObj => {
        const sizeMB = (fileObj.file.size / (1024 * 1024)).toFixed(2);
        
        let statusIcon = '<div class="w-2 h-2 rounded-full bg-gray-500"></div>';
        let statusText = 'Queued';
        
        if (fileObj.status === 'processing') {
            statusIcon = '<div class="w-3 h-3 border-2 border-blue-500 border-t-transparent rounded-full animate-spin"></div>';
            statusText = 'Processing...';
        } else if (fileObj.status === 'completed') {
            statusIcon = '<svg class="w-3 h-3 text-green-500" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 13l4 4L19 7"></path></svg>';
            statusText = 'Completed';
        } else if (fileObj.status === 'failed') {
            statusIcon = '<svg class="w-3 h-3 text-red-500" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12"></path></svg>';
            statusText = 'Failed';
        }

        const li = document.createElement('div');
        li.className = 'flex items-center justify-between p-2 bg-gray-800 rounded border border-gray-700 text-xs';
        
        li.innerHTML = `
            <div class="flex items-center space-x-2 truncate flex-grow">
                ${statusIcon}
                <div class="truncate">
                    <div class="font-medium text-gray-200 truncate">${fileObj.file.name}</div>
                    <div class="text-gray-500">${sizeMB} MB • ${statusText}</div>
                    ${fileObj.error ? `<div class="text-red-400 mt-1 whitespace-normal break-words">${fileObj.error}</div>` : ''}
                </div>
            </div>
            ${fileObj.status === 'queued' ? `<button onclick="removeFile('${fileObj.id}')" class="text-gray-500 hover:text-red-400 ml-2 p-1"><svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12"></path></svg></button>` : ''}
        `;
        
        uploadQueue.appendChild(li);
    });
}

// Batch Processing
processBatchBtn.addEventListener('click', async () => {
    const caseId = caseIdInput.value.trim();
    if (!caseId) {
        alert("You must provide an Active Case ID before processing documents.");
        caseIdInput.focus();
        return;
    }

    if (filesToUpload.length === 0) return;

    isProcessing = true;
    processBatchBtn.disabled = true;
    processBatchBtn.classList.add('opacity-50', 'cursor-not-allowed');
    processBatchBtn.innerHTML = '<span class="flex items-center justify-center"><svg class="animate-spin -ml-1 mr-3 h-5 w-5 text-white" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24"><circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle><path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path></svg>Processing...</span>';

    // Mark all queued as processing visually
    filesToUpload.forEach(f => {
        if (f.status === 'queued') f.status = 'processing';
    });
    renderQueue();

    const formData = new FormData();
    formData.append('case_id', caseId);
    filesToUpload.forEach(f => {
        formData.append('document', f.file);
    });

    try {
        const response = await fetch('/api/ingest/batch', {
            method: 'POST',
            body: formData
        });

        if (!response.ok) {
            throw new Error(await response.text());
        }

        const resultData = await response.json();
        
        // Update file states based on results
        if (resultData.results) {
            resultData.results.forEach(res => {
                // Find matching file by name
                const f = filesToUpload.find(file => file.file.name === res.file_name);
                if (f) {
                    f.status = res.status;
                    f.error = res.error;
                }
            });
            renderQueue();
            
            // Reload case details to show new report
            await loadCaseDetails(caseId);
            
            // System message
            appendChat('System', `Batch processing complete for Case ${caseId}. Processed ${resultData.results.length} files.`, 'system');
        }

    } catch (error) {
        alert(`Batch Processing Error: ${error.message}`);
        // Reset processing states
        filesToUpload.forEach(f => {
            if (f.status === 'processing') {
                f.status = 'failed';
                f.error = 'Upload request failed';
            }
        });
        renderQueue();
    } finally {
        isProcessing = false;
        processBatchBtn.disabled = false;
        processBatchBtn.classList.remove('opacity-50', 'cursor-not-allowed');
        processBatchBtn.innerText = 'Process Batch';
        loadCases(); // Refresh list
    }
});

// Case Management
async function loadCases() {
    try {
        const res = await fetch('/api/cases');
        const cases = await res.json();
        caseList.innerHTML = '';
        
        cases.forEach(c => {
            const li = document.createElement('li');
            li.className = "p-3 bg-gray-800 hover:bg-gray-700 rounded-lg cursor-pointer transition-colors border border-gray-700 hover:border-blue-500 group";
            li.innerHTML = `
                <div class="font-bold text-gray-200 group-hover:text-blue-400 transition-colors">${c.case_id}</div>
                <div class="text-xs text-gray-500 mt-1">${new Date(c.created_at).toLocaleString()}</div>
            `;
            li.onclick = () => loadCaseDetails(c.case_id);
            caseList.appendChild(li);
        });
    } catch (e) {
        console.error("Failed to load cases", e);
    }
}

async function loadCaseDetails(caseId) {
    caseIdInput.value = caseId;
    try {
        const res = await fetch(`/api/cases/${caseId}`);
        if (!res.ok) throw new Error("Failed to load case");
        const caseData = await res.json();
        
        caseDetailsEmpty.classList.add('hidden');
        caseDetailsContent.classList.remove('hidden');
        
        if (caseData.adjudication_report) {
            reportOutput.innerText = JSON.stringify(caseData.adjudication_report, null, 2);
        } else {
            reportOutput.innerText = "No adjudication report available yet.";
        }

        // Clear chat & hide empty state
        chatHistory.innerHTML = '';
        if (loadOlderContainer) {
            loadOlderContainer.classList.add('hidden');
            chatHistory.appendChild(loadOlderContainer);
        }
        if (emptyState) emptyState.remove();
        
        // Populate document text
        documentTextArea.value = caseData.document_text || "";

        // Load History
        await loadChatHistory(caseId);

        appendChat('System', `Case ${caseId} loaded. System armed and ready for interrogation.`, 'system');

    } catch (e) {
        alert(e.message);
    }
}

// Chat Functionality
function setupChatInput() {
    chatInput.addEventListener('keydown', (e) => {
        if (e.key === 'Enter' && !e.shiftKey) {
            e.preventDefault();
            sendChatMessage();
        }
    });

    chatInput.addEventListener('input', function() {
        this.style.height = 'auto';
        this.style.height = (this.scrollHeight) + 'px';
    });

    sendChatBtn.addEventListener('click', sendChatMessage);
}

async function sendChatMessage() {
    const caseId = caseIdInput.value.trim();
    const message = chatInput.value.trim();

    if (!caseId) {
        alert("You must select or define an Active Case ID before interrogating.");
        caseIdInput.focus();
        return;
    }
    if (!message) return;

    // Reset input
    chatInput.value = '';
    chatInput.style.height = 'auto';
    
    if (emptyState) emptyState.remove();

    appendChat('You', message, 'user');

    const typingId = 'typing-' + Date.now();
    appendChat('D3-Agent', '<div class="flex items-center space-x-2"><div class="w-2 h-2 bg-gray-500 rounded-full animate-bounce"></div><div class="w-2 h-2 bg-gray-500 rounded-full animate-bounce" style="animation-delay: 0.2s"></div><div class="w-2 h-2 bg-gray-500 rounded-full animate-bounce" style="animation-delay: 0.4s"></div></div>', 'typing', typingId);

    try {
        const response = await fetch('/api/chat', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ case_id: caseId, query: message })
        });

        let result = await response.text();
        
        // Try parsing JSON if API returns it
        try {
            const jsonRes = JSON.parse(result);
            if (jsonRes.answer) {
                result = jsonRes.answer;
            }
        } catch(e) {
            // keep as text
        }

        document.getElementById(typingId).remove();
        appendChat('D3-Agent', result, 'agent');

    } catch (error) {
        document.getElementById(typingId).remove();
        appendChat('System Error', error.message, 'error');
    }
}

async function loadChatHistory(caseId, beforeId = null) {
    if (historyLoading) return;
    historyLoading = true;
    
    console.log(`[History] Fetching history for case: ${caseId}, beforeId: ${beforeId}`);
    
    try {
        let url = `/api/cases/${encodeURIComponent(caseId)}/history?limit=30`;
        if (beforeId) url += `&before_id=${beforeId}`;
        
        const res = await fetch(url);
        if (!res.ok) {
            const errorText = await res.text();
            throw new Error(`API Error: ${errorText}`);
        }
        const messages = await res.json();
        console.log(`[History] Received ${messages.length} messages.`);
        
        if (!beforeId) {
            // First load
            oldestMessageId = null;
        }

        if (messages.length > 0) {
            // Messages arrive oldest-first from API (ASC)
            // due to sub-query reversal in postgres.rs
            
            // Track the oldest message for "Load More"
            oldestMessageId = messages[0].id;
            
            // For UI display, we want them chronological (bottom is newest).
            // Since they are already ASC, we just iterate.
            messages.forEach(msg => {
                appendChat(
                    msg.role === 'user' ? 'You' : 'D3-Agent', 
                    msg.content, 
                    msg.role === 'assistant' ? 'agent' : 'user', 
                    `msg-${msg.id}`,
                    beforeId // prepend if loading older
                );
            });

            // Show "Load More" if we got a full page
            if (messages.length === 30) {
                loadOlderContainer.classList.remove('hidden');
            } else {
                loadOlderContainer.classList.add('hidden');
            }
        } else {
            if (!beforeId) {
                // No messages at all
                loadOlderContainer.classList.add('hidden');
            }
        }
    } catch (e) {
        console.error("Chat History Error:", e);
        appendChat('System Error', `Failed to load conversation history: ${e.message}`, 'error');
    } finally {
        historyLoading = false;
    }
}

function setupActionButtons() {
    clearHistoryBtn.onclick = async () => {
        const caseId = caseIdInput.value.trim();
        if (!caseId) return;
        if (!confirm("Wipe all chat history for this case?")) return;
        
        try {
            const res = await fetch(`/api/cases/${caseId}/history`, { method: 'DELETE' });
            if (res.ok) {
                chatHistory.innerHTML = '';
                chatHistory.appendChild(loadOlderContainer);
                loadOlderContainer.classList.add('hidden');
                appendChat('System', 'Chat history cleared.', 'system');
            }
        } catch(e) { alert(e.message); }
    };

    loadOlderBtn.onclick = () => {
        const caseId = caseIdInput.value.trim();
        if (caseId && oldestMessageId) {
            loadChatHistory(caseId, oldestMessageId);
        }
    };

    deleteCaseBtn.onclick = async () => {
        const caseId = caseIdInput.value.trim();
        if (!caseId) return;
        if (!confirm(`Permanently DELETE case ${caseId} and all associated data?`)) return;
        
        try {
            const res = await fetch(`/api/cases/${caseId}`, { method: 'DELETE' });
            if (res.ok) {
                window.location.reload();
            }
        } catch(e) { alert(e.message); }
    };

    readjudicateBtn.onclick = async () => {
        const caseId = caseIdInput.value.trim();
        if (!caseId) return;
        
        readjudicateBtn.disabled = true;
        const originalText = readjudicateBtn.innerHTML;
        readjudicateBtn.innerHTML = 'Processing...';
        
        try {
            const res = await fetch(`/api/cases/${caseId}/readjudicate`, { method: 'POST' });
            if (!res.ok) throw new Error(await res.text());
            await loadCaseDetails(caseId);
            appendChat('System', 'Readjudication complete.', 'system');
        } catch(e) { alert(e.message); }
        finally {
            readjudicateBtn.disabled = false;
            readjudicateBtn.innerHTML = originalText;
        }
    };

    updateTextBtn.onclick = async () => {
        const caseId = caseIdInput.value.trim();
        if (!caseId) return;
        
        try {
            const res = await fetch(`/api/cases/${caseId}`, {
                method: 'PUT',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ document_text: documentTextArea.value })
            });
            if (res.ok) {
                appendChat('System', 'Document text updated.', 'system');
            } else {
                throw new Error(await res.text());
            }
        } catch(e) { alert(e.message); }
    };
}

async function deleteMessage(msgId) {
    if (!confirm("Delete this message?")) return;
    try {
        const res = await fetch(`/api/messages/${msgId}`, { method: 'DELETE' });
        if (res.ok) {
            document.getElementById(`msg-${msgId}`).remove();
        }
    } catch(e) { alert(e.message); }
}

function appendChat(sender, text, type, id = null, prepend = false) {
    const msgDiv = document.createElement('div');
    if (id) msgDiv.id = id;
    
    const isMarkdown = type === 'agent' && renderToggle.checked;

    let contentHtml = text;
    
    if (isMarkdown) {
        const rawHtml = marked.parse(text);
        contentHtml = DOMPurify.sanitize(rawHtml);
    } else if (type !== 'typing') {
        // Escape basic HTML for raw text
        const temp = document.createElement('div');
        temp.textContent = text;
        contentHtml = temp.innerHTML.replace(/\n/g, '<br>');
    }

    let bubbleClasses = 'p-4 rounded-xl text-sm leading-relaxed msg-bubble ';
    let wrapperClasses = 'flex flex-col ';

    if (type === 'user') {
        bubbleClasses += 'bg-blue-600 text-white rounded-br-none self-end';
        wrapperClasses += 'items-end';
    } else if (type === 'agent') {
        bubbleClasses += 'bg-gray-800 text-gray-200 border border-gray-700 rounded-bl-none self-start';
        if (isMarkdown) bubbleClasses += ' markdown-body';
        wrapperClasses += 'items-start';
    } else if (type === 'system') {
        bubbleClasses += 'bg-gray-900/50 text-green-400 font-mono text-xs border border-green-900/50 self-center mx-auto text-center';
        wrapperClasses += 'items-center';
    } else if (type === 'error') {
        bubbleClasses += 'bg-red-900/50 text-red-400 border border-red-800 rounded-bl-none self-start';
        wrapperClasses += 'items-start';
    } else if (type === 'typing') {
        bubbleClasses += 'bg-gray-800 text-gray-400 border border-gray-700 rounded-bl-none self-start py-3 px-4';
        wrapperClasses += 'items-start';
    }

    const senderHtml = type !== 'system' && type !== 'typing' ? `<span class="text-xs font-bold text-gray-500 mb-1 px-1 ${type === 'user' ? 'text-right' : 'text-left'}">${sender}</span>` : '';

    msgDiv.className = wrapperClasses + ' group/msg'; // Add group for hover effects
    
    let deleteBtn = '';
    if (id && id.startsWith('msg-') && type !== 'system') {
        const rawId = id.replace('msg-', '');
        deleteBtn = `
            <button onclick="deleteMessage(${rawId})" class="opacity-0 group-hover/msg:opacity-100 text-[10px] text-gray-600 hover:text-red-500 transition-opacity ml-2 self-center">
                Delete
            </button>
        `;
    }

    msgDiv.innerHTML = `
        ${senderHtml}
        <div class="flex items-center">
            <div class="${bubbleClasses}">${contentHtml}</div>
            ${type === 'agent' ? deleteBtn : ''}
            ${type === 'user' ? `<div class="order-first mr-2">${deleteBtn}</div>` : ''}
        </div>
    `;

    if (prepend) {
        // Insert after the loadOlderContainer
        if (loadOlderContainer.nextSibling) {
            chatHistory.insertBefore(msgDiv, loadOlderContainer.nextSibling);
        } else {
            chatHistory.appendChild(msgDiv);
        }
    } else {
        chatHistory.appendChild(msgDiv);
    }
    
    // Add copy buttons to code blocks if rendered as markdown
    if (isMarkdown) {
        const preBlocks = msgDiv.querySelectorAll('pre');
        preBlocks.forEach(pre => {
            const btn = document.createElement('button');
            btn.className = 'copy-code-btn';
            btn.innerText = 'Copy';
            btn.onclick = () => {
                const code = pre.querySelector('code').innerText;
                navigator.clipboard.writeText(code);
                btn.innerText = 'Copied!';
                setTimeout(() => btn.innerText = 'Copy', 2000);
            };
            pre.appendChild(btn);
        });
    }

    if (!prepend) {
        chatHistory.scrollTop = chatHistory.scrollHeight;
    }
}

// Handle toggle rendering retroactively (optional, but good UX)
renderToggle.addEventListener('change', () => {
    // For a real app, you might want to re-render all agent messages
    // but for simplicity, we'll just let it affect new messages
});
