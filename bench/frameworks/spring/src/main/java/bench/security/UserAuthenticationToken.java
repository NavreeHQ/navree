package bench.security;

import bench.user.User;
import java.util.List;
import java.util.Map;
import org.springframework.security.oauth2.jwt.Jwt;
import org.springframework.security.oauth2.server.resource.authentication.AbstractOAuth2TokenAuthenticationToken;

/** A verified bearer JWT whose principal is the {@link User} row it names. */
public final class UserAuthenticationToken extends AbstractOAuth2TokenAuthenticationToken<Jwt> {

    public UserAuthenticationToken(Jwt jwt, User user) {
        super(jwt, user, jwt, List.of());
        setAuthenticated(true);
    }

    @Override
    public Map<String, Object> getTokenAttributes() {
        return getToken().getClaims();
    }

    @Override
    public String getName() {
        return getToken().getSubject();
    }
}
