import { Terminal } from 'xterm';
import { FitAddon } from 'xterm-addon-fit';
import { AttachAddon } from 'xterm-addon-attach';

// Create an xterm.js terminal and attach it to a DOM element
let term = new Terminal();
let fitAddon = new FitAddon();
term.loadAddon(fitAddon);
let container = document.getElementById('terminal');
term.open(container);

// Fit the terminal to the size of the container
fitAddon.fit();

// Connect to the WebSocket server
let socket = new WebSocket('ws://localhost:8080/ws/container_id');
let attachAddon = new AttachAddon(socket);
term.loadAddon(attachAddon);

// Send resize messages to the server when the terminal size changes
term.onResize(({ cols, rows }) => {
  let size = cols + ':' + rows;
  socket.send(size);
});
