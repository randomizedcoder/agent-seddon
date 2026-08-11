// FAIL fixture (fail-metrics): relay_published_total is hardwired to zero
// Reference solution for the relay objective — the PASS fixture (R13).
package main

import (
	"bufio"
	"flag"
	"fmt"
	"net"
	"net/http"
	"os"
	"strconv"
	"strings"
	"sync"
	"time"
)

type server struct {
	tokens map[string]bool
	rate   int // per-connection PUB budget: burst + refill/sec; 0 = unlimited

	mu        sync.Mutex
	subs      map[string]map[*client]bool // topic -> subscribers
	journal   *os.File
	pubTotal  int64
	connTotal int64
}

type client struct {
	conn net.Conn
	wmu  sync.Mutex // MSG fan-out and command replies interleave

	tokens float64 // rate-limit bucket
	last   time.Time
}

func (c *client) reply(line string) {
	c.wmu.Lock()
	defer c.wmu.Unlock()
	fmt.Fprintf(c.conn, "%s\n", line)
}

func main() {
	listen := flag.String("listen", "127.0.0.1:0", "TCP listen address")
	tokensPath := flag.String("tokens", "", "file of valid auth tokens, one per line")
	journalPath := flag.String("journal", "", "append `<topic> <text>` per accepted PUB")
	metricsAddr := flag.String("metrics", "", "HTTP metrics listen address")
	rate := flag.Int("rate", 0, "per-connection PUB budget (burst, refill/sec); 0 = off")
	flag.Parse()

	s := &server{
		tokens: map[string]bool{},
		rate:   *rate,
		subs:   map[string]map[*client]bool{},
	}
	data, err := os.ReadFile(*tokensPath)
	if err != nil {
		fmt.Fprintf(os.Stderr, "reading tokens: %v\n", err)
		os.Exit(1)
	}
	for _, line := range strings.Split(string(data), "\n") {
		if t := strings.TrimSpace(line); t != "" {
			s.tokens[t] = true
		}
	}
	if *journalPath != "" {
		f, err := os.OpenFile(*journalPath, os.O_CREATE|os.O_APPEND|os.O_WRONLY, 0o644)
		if err != nil {
			fmt.Fprintf(os.Stderr, "opening journal: %v\n", err)
			os.Exit(1)
		}
		s.journal = f
	}

	ln, err := net.Listen("tcp", *listen)
	if err != nil {
		fmt.Fprintf(os.Stderr, "listen: %v\n", err)
		os.Exit(1)
	}
	fmt.Printf("LISTENING %s\n", ln.Addr())

	if *metricsAddr != "" {
		mln, err := net.Listen("tcp", *metricsAddr)
		if err != nil {
			fmt.Fprintf(os.Stderr, "metrics listen: %v\n", err)
			os.Exit(1)
		}
		fmt.Printf("METRICS %s\n", mln.Addr())
		mux := http.NewServeMux()
		mux.HandleFunc("/metrics", func(w http.ResponseWriter, _ *http.Request) {
			s.mu.Lock()
			pub, conns := int64(0), s.connTotal
			s.mu.Unlock()
			fmt.Fprintf(w, "relay_published_total %d\nrelay_connections_total %d\n", pub, conns)
		})
		go func() { _ = http.Serve(mln, mux) }()
	}

	for {
		conn, err := ln.Accept()
		if err != nil {
			continue
		}
		s.mu.Lock()
		s.connTotal++
		s.mu.Unlock()
		go s.handle(conn)
	}
}

func (s *server) handle(conn net.Conn) {
	c := &client{conn: conn, tokens: float64(s.rate), last: time.Now()}
	defer func() {
		s.unsubscribeAll(c)
		_ = conn.Close()
	}()
	sc := bufio.NewScanner(conn)

	// Auth gate: the first line must be a valid AUTH.
	if !sc.Scan() {
		return
	}
	tok, ok := strings.CutPrefix(sc.Text(), "AUTH ")
	if !ok || !s.tokens[strings.TrimSpace(tok)] {
		c.reply("ERR unauthorized")
		return
	}
	c.reply("OK")

	for sc.Scan() {
		line := sc.Text()
		switch {
		case line == "PING":
			c.reply("PONG")
		case strings.HasPrefix(line, "SUB "):
			topic := strings.TrimSpace(strings.TrimPrefix(line, "SUB "))
			if topic == "" || strings.Contains(topic, " ") {
				c.reply("ERR bad command")
				continue
			}
			s.subscribe(topic, c)
			c.reply("OK")
		case strings.HasPrefix(line, "PUB "):
			topic, text, ok := strings.Cut(strings.TrimPrefix(line, "PUB "), " ")
			if !ok || topic == "" || text == "" {
				c.reply("ERR bad command")
				continue
			}
			if !s.allow(c) {
				c.reply("ERR rate limited")
				continue
			}
			s.publish(topic, text)
			c.reply("OK")
		case strings.HasPrefix(line, "REPLAY "):
			rest := strings.TrimPrefix(line, "REPLAY ")
			topic, nStr, ok := strings.Cut(rest, " ")
			n, err := strconv.Atoi(strings.TrimSpace(nStr))
			if !ok || topic == "" || err != nil || n < 0 {
				c.reply("ERR bad command")
				continue
			}
			for _, msg := range s.replay(topic, n) {
				c.reply(fmt.Sprintf("MSG %s %s", topic, msg))
			}
			c.reply("OK")
		default:
			c.reply("ERR bad command")
		}
	}
	// A read error is a hostile or vanished client — that connection ends,
	// the server keeps serving.
	_ = sc.Err()
}

// allow spends one token from the per-connection bucket (burst rate,
// refill rate/sec); rate 0 disables limiting.
func (s *server) allow(c *client) bool {
	if s.rate == 0 {
		return true
	}
	now := time.Now()
	c.tokens += now.Sub(c.last).Seconds() * float64(s.rate)
	c.last = now
	if c.tokens > float64(s.rate) {
		c.tokens = float64(s.rate)
	}
	if c.tokens < 1 {
		return false
	}
	c.tokens--
	return true
}

func (s *server) subscribe(topic string, c *client) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.subs[topic] == nil {
		s.subs[topic] = map[*client]bool{}
	}
	s.subs[topic][c] = true
}

func (s *server) unsubscribeAll(c *client) {
	s.mu.Lock()
	defer s.mu.Unlock()
	for _, m := range s.subs {
		delete(m, c)
	}
}

// publish journals (flushed — accepted messages survive a kill) and fans out.
func (s *server) publish(topic, text string) {
	s.mu.Lock()
	s.pubTotal++
	if s.journal != nil {
		fmt.Fprintf(s.journal, "%s %s\n", topic, text)
		_ = s.journal.Sync()
	}
	targets := make([]*client, 0, len(s.subs[topic]))
	for sub := range s.subs[topic] {
		targets = append(targets, sub)
	}
	s.mu.Unlock()
	for _, sub := range targets {
		sub.reply(fmt.Sprintf("MSG %s %s", topic, text))
	}
}

// replay reads the journal file for up to n most recent messages on topic,
// oldest first.
func (s *server) replay(topic string, n int) []string {
	s.mu.Lock()
	path := ""
	if s.journal != nil {
		path = s.journal.Name()
	}
	s.mu.Unlock()
	if path == "" {
		return nil
	}
	data, err := os.ReadFile(path)
	if err != nil {
		return nil
	}
	var msgs []string
	for _, line := range strings.Split(string(data), "\n") {
		if text, ok := strings.CutPrefix(line, topic+" "); ok {
			msgs = append(msgs, text)
		}
	}
	if len(msgs) > n {
		msgs = msgs[len(msgs)-n:]
	}
	return msgs
}
