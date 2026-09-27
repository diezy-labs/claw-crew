package interceptors

import (
	"context"
	"errors"
	"log/slog"
	"time"

	appErrors "github.com/diezy-labs/claw-crew/engine/core/errors"
	"github.com/diezy-labs/claw-crew/engine/core/logger"
	"github.com/diezy-labs/claw-crew/engine/core/metrics"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

// UnaryServerInterceptor provides logging, metrics, and error translation for unary RPCs
func UnaryServerInterceptor() grpc.UnaryServerInterceptor {
	return func(
		ctx context.Context,
		req any,
		info *grpc.UnaryServerInfo,
		handler grpc.UnaryHandler,
	) (any, error) {
		start := time.Now()
		log := logger.Get()

		resp, err := handler(ctx, req)
		duration := time.Since(start).Seconds()

		metrics.GRPCRequestDuration.WithLabelValues(info.FullMethod).Observe(duration)

		if err != nil {
			statusCode := codes.Unknown
			var appErr *appErrors.AppError
			if errors.As(err, &appErr) {
				metrics.ErrorsTotal.WithLabelValues(string(appErr.Layer), string(appErr.Code)).Inc()
			}

			grpcErr := appErrors.ToGRPCStatus(err)
			if st, ok := status.FromError(grpcErr); ok {
				statusCode = st.Code()
			}

			metrics.GRPCRequestsTotal.WithLabelValues(info.FullMethod, statusCode.String()).Inc()

			log.ErrorContext(ctx, "grpc unary request failed",
				slog.String("method", info.FullMethod),
				slog.Float64("duration_sec", duration),
				slog.String("status_code", statusCode.String()),
				slog.String("error", err.Error()),
			)
			return nil, grpcErr
		}

		metrics.GRPCRequestsTotal.WithLabelValues(info.FullMethod, codes.OK.String()).Inc()

		log.DebugContext(ctx, "grpc unary request completed",
			slog.String("method", info.FullMethod),
			slog.Float64("duration_sec", duration),
			slog.String("status_code", codes.OK.String()),
		)

		return resp, nil
	}
}

// StreamServerInterceptor provides logging, metrics, and error translation for streaming RPCs
func StreamServerInterceptor() grpc.StreamServerInterceptor {
	return func(
		srv any,
		ss grpc.ServerStream,
		info *grpc.StreamServerInfo,
		handler grpc.StreamHandler,
	) error {
		start := time.Now()
		log := logger.Get()
		ctx := ss.Context()

		log.InfoContext(ctx, "grpc stream started",
			slog.String("method", info.FullMethod),
		)

		err := handler(srv, ss)
		duration := time.Since(start).Seconds()

		metrics.GRPCRequestDuration.WithLabelValues(info.FullMethod).Observe(duration)

		if err != nil {
			statusCode := codes.Unknown
			var appErr *appErrors.AppError
			if errors.As(err, &appErr) {
				metrics.ErrorsTotal.WithLabelValues(string(appErr.Layer), string(appErr.Code)).Inc()
			}

			grpcErr := appErrors.ToGRPCStatus(err)
			if st, ok := status.FromError(grpcErr); ok {
				statusCode = st.Code()
			}

			metrics.GRPCRequestsTotal.WithLabelValues(info.FullMethod, statusCode.String()).Inc()

			log.ErrorContext(ctx, "grpc stream failed",
				slog.String("method", info.FullMethod),
				slog.Float64("duration_sec", duration),
				slog.String("status_code", statusCode.String()),
				slog.String("error", err.Error()),
			)
			return grpcErr
		}

		metrics.GRPCRequestsTotal.WithLabelValues(info.FullMethod, codes.OK.String()).Inc()

		log.InfoContext(ctx, "grpc stream completed",
			slog.String("method", info.FullMethod),
			slog.Float64("duration_sec", duration),
			slog.String("status_code", codes.OK.String()),
		)

		return nil
	}
}
