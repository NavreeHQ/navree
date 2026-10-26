// Gin + pgx + golang-jwt + Gin's binding (go-playground/validator), see bench/SPEC.md.
package main

import (
	"context"
	"errors"
	"log"
	"math"
	"math/rand/v2"
	"net/http"
	"net/url"
	"os"
	"runtime"
	"strconv"
	"strings"
	"time"

	"github.com/gin-gonic/gin"
	"github.com/golang-jwt/jwt/v5"
	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
)

const maxDBConns = 64

type world struct {
	ID           int32 `json:"id"`
	RandomNumber int32 `json:"randomNumber"`
}

type user struct {
	ID   uuid.UUID `json:"id"`
	Name string    `json:"name"`
}

type deal struct {
	ID          uuid.UUID `json:"id"`
	Title       string    `json:"title"`
	AmountCents int64     `json:"amount_cents"`
	Stage       string    `json:"stage"`
	Owner       user      `json:"owner"`
	CreatedAt   time.Time `json:"created_at"`
}

type pageMeta struct {
	CurrentPage int   `json:"current_page"`
	PerPage     int   `json:"per_page"`
	Total       int64 `json:"total"`
	LastPage    int64 `json:"last_page"`
}

type dealPage struct {
	Data []deal   `json:"data"`
	Meta pageMeta `json:"meta"`
}

type listDealsQuery struct {
	Page    int `form:"page,default=1" binding:"min=1"`
	PerPage int `form:"per_page,default=20" binding:"min=1,max=100"`
}

type createDealBody struct {
	Title       string `json:"title" binding:"required,min=1,max=200"`
	AmountCents *int64 `json:"amount_cents" binding:"required,min=0,max=1000000000000"`
	Stage       string `json:"stage" binding:"required,oneof=lead qualified proposal negotiation won lost"`
}

const userKey = "user"

type server struct {
	db        *pgxpool.Pool
	jwtParser *jwt.Parser
	jwtSecret []byte
}

func main() {
	pool, err := connect()
	if err != nil {
		log.Fatalf("database: %v", err)
	}
	defer pool.Close()

	issuer := os.Getenv("JWT_ISSUER")
	if issuer == "" {
		issuer = "keel"
	}
	s := &server{
		db: pool,
		jwtParser: jwt.NewParser(
			jwt.WithValidMethods([]string{jwt.SigningMethodHS256.Alg()}),
			jwt.WithIssuer(issuer),
			jwt.WithExpirationRequired(),
		),
		jwtSecret: []byte(os.Getenv("JWT_SECRET")),
	}

	r := gin.New()
	r.Use(gin.Recovery())
	if err := r.SetTrustedProxies(nil); err != nil {
		log.Fatal(err)
	}

	r.GET("/json", s.json)
	r.GET("/db", s.singleQuery)
	r.GET("/queries", s.multipleQueries)

	api := r.Group("/api", s.authenticate)
	api.GET("/deals", s.listDeals)
	api.POST("/deals", s.createDeal)

	log.Printf("gin %s, %s, GOMAXPROCS=%d, NumCPU=%d, db pool max %d",
		gin.Version, runtime.Version(), runtime.GOMAXPROCS(0), runtime.NumCPU(), pool.Config().MaxConns)
	if err := r.Run("0.0.0.0:8080"); err != nil {
		log.Fatal(err)
	}
}

// connect opens the pgx pool from DATABASE_URL (or the DB_* variables).
func connect() (*pgxpool.Pool, error) {
	dsn := os.Getenv("DATABASE_URL")
	if dsn == "" {
		dsn = (&url.URL{
			Scheme: "postgres",
			User:   url.UserPassword(os.Getenv("DB_USER"), os.Getenv("DB_PASSWORD")),
			Host:   os.Getenv("DB_HOST") + ":" + os.Getenv("DB_PORT"),
			Path:   os.Getenv("DB_NAME"),
		}).String()
	}
	cfg, err := pgxpool.ParseConfig(dsn)
	if err != nil {
		return nil, err
	}
	cfg.MaxConns = maxDBConns
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	return pgxpool.NewWithConfig(ctx, cfg)
}

func (s *server) json(c *gin.Context) {
	c.JSON(http.StatusOK, gin.H{"message": "Hello, World!"})
}

func (s *server) randomWorld(c *gin.Context) (world, error) {
	var w world
	err := s.db.QueryRow(c.Request.Context(), "SELECT id, randomnumber FROM world WHERE id = $1", rand.IntN(10000)+1).
		Scan(&w.ID, &w.RandomNumber)
	return w, err
}

func (s *server) singleQuery(c *gin.Context) {
	w, err := s.randomWorld(c)
	if err != nil {
		serverError(c, err)
		return
	}
	c.JSON(http.StatusOK, w)
}

func (s *server) multipleQueries(c *gin.Context) {
	n, err := strconv.Atoi(c.Query("queries"))
	if err != nil || n < 1 {
		n = 1
	} else if n > 500 {
		n = 500
	}
	worlds := make([]world, n)
	for i := range worlds {
		if worlds[i], err = s.randomWorld(c); err != nil {
			serverError(c, err)
			return
		}
	}
	c.JSON(http.StatusOK, worlds)
}

// authenticate checks the bearer JWT (HS256, exp, iss) and loads the user row.
func (s *server) authenticate(c *gin.Context) {
	raw, ok := strings.CutPrefix(c.GetHeader("Authorization"), "Bearer ")
	if !ok {
		unauthenticated(c)
		return
	}
	var claims jwt.RegisteredClaims
	if _, err := s.jwtParser.ParseWithClaims(raw, &claims, func(*jwt.Token) (any, error) { return s.jwtSecret, nil }); err != nil {
		unauthenticated(c)
		return
	}
	id, err := uuid.Parse(claims.Subject)
	if err != nil {
		unauthenticated(c)
		return
	}
	var u user
	err = s.db.QueryRow(c.Request.Context(), "SELECT id, name FROM users WHERE id = $1", id).Scan(&u.ID, &u.Name)
	if errors.Is(err, pgx.ErrNoRows) {
		unauthenticated(c)
		return
	}
	if err != nil {
		serverError(c, err)
		return
	}
	c.Set(userKey, u)
	c.Next()
}

func (s *server) listDeals(c *gin.Context) {
	var q listDealsQuery
	if err := c.ShouldBindQuery(&q); err != nil {
		validationFailed(c, err)
		return
	}
	ctx := c.Request.Context()

	var total int64
	if err := s.db.QueryRow(ctx, "SELECT count(*) FROM deals").Scan(&total); err != nil {
		serverError(c, err)
		return
	}

	data := []deal{}
	// A page so far out that its offset overflows is empty anyway.
	if q.Page-1 <= math.MaxInt64/q.PerPage {
		rows, err := s.db.Query(ctx,
			`SELECT d.id, d.title, d.amount_cents, d.stage, d.created_at, u.id, u.name
			   FROM deals d JOIN users u ON u.id = d.owner_id
			  ORDER BY d.created_at DESC, d.id DESC
			  LIMIT $1 OFFSET $2`,
			q.PerPage, int64(q.Page-1)*int64(q.PerPage))
		if err != nil {
			serverError(c, err)
			return
		}
		data, err = pgx.AppendRows(data, rows, func(row pgx.CollectableRow) (deal, error) {
			var d deal
			err := row.Scan(&d.ID, &d.Title, &d.AmountCents, &d.Stage, &d.CreatedAt, &d.Owner.ID, &d.Owner.Name)
			return d, err
		})
		if err != nil {
			serverError(c, err)
			return
		}
	}

	lastPage := (total + int64(q.PerPage) - 1) / int64(q.PerPage)
	c.JSON(http.StatusOK, dealPage{
		Data: data,
		Meta: pageMeta{CurrentPage: q.Page, PerPage: q.PerPage, Total: total, LastPage: max(1, lastPage)},
	})
}

func (s *server) createDeal(c *gin.Context) {
	var body createDealBody
	if err := c.ShouldBindJSON(&body); err != nil {
		validationFailed(c, err)
		return
	}
	owner := c.MustGet(userKey).(user)
	d := deal{
		ID:          uuid.New(),
		Title:       body.Title,
		AmountCents: *body.AmountCents,
		Stage:       body.Stage,
		Owner:       owner,
		CreatedAt:   time.Now().UTC().Truncate(time.Microsecond),
	}
	_, err := s.db.Exec(c.Request.Context(),
		`INSERT INTO deals (id, title, amount_cents, stage, owner_id, created_at, updated_at)
		 VALUES ($1, $2, $3, $4, $5, $6, $6)`,
		d.ID, d.Title, d.AmountCents, d.Stage, owner.ID, d.CreatedAt)
	if err != nil {
		serverError(c, err)
		return
	}
	c.JSON(http.StatusCreated, d)
}

func unauthenticated(c *gin.Context) {
	c.AbortWithStatusJSON(http.StatusUnauthorized, gin.H{"message": "Unauthenticated."})
}

func validationFailed(c *gin.Context, err error) {
	c.AbortWithStatusJSON(http.StatusBadRequest, gin.H{"message": "Validation failed", "error": err.Error()})
}

func serverError(c *gin.Context, err error) {
	if !errors.Is(err, context.Canceled) { // not when the client went away
		log.Printf("%s %s: %v", c.Request.Method, c.Request.URL.Path, err)
	}
	c.AbortWithStatusJSON(http.StatusInternalServerError, gin.H{"message": "Internal Server Error"})
}
